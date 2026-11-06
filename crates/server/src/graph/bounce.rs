//! Bounce to audio on the session's actor (RENDER-40): planned on the accepted revision, and put
//! in place as one actor job, one undo step. The render runs between them, off the actor
//! (`crate::bounce`), as a stem's does.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use m2_editor::bounce::{Bounce, Bounced};
use m2_editor::render::{Render, Threads};
use m2_song::AssetName;
use serde_json::{Value, json};

use super::Graph;

/// A bounce planned on the accepted revision: the bounce, its renderer, the new file's name, the
/// track's label and the song's folder.
pub struct BouncePlan {
    pub bounce: Bounce,
    pub render: Render,
    pub asset: AssetName,
    pub label: String,
    pub dir: PathBuf,
}

/// `<track>-bounce.wav`, else `<track>-bounce-2.wav`…: the first name in the song's `assets/` that
/// neither the file nor its provenance takes.
fn free_name(dir: &Path, track: &str) -> AssetName {
    let taken = |n: &str| ["", ".json"].iter().any(|e| dir.join(m2_song::path::ASSETS).join(format!("{n}{e}")).symlink_metadata().is_ok());
    (1..)
        .map(|n| if n == 1 { format!("{track}-bounce.wav") } else { format!("{track}-bounce-{n}.wav") })
        .find(|n| !taken(n))
        .and_then(|n| AssetName::new(&n).ok())
        .expect("a track's key makes a file name")
}

impl Graph {
    /// Plan a bounce of track `track` of the open song, as the window with session `token` asked:
    /// refused in words for what can't be bounced in place, and while the code doesn't compile.
    /// Into `assets/<name>.wav` when a name is given (`m2 bounce`'s), else a free one. Only
    /// planning it waits for the session.
    pub fn bounce_plan(&self, token: u64, track: String, name: Option<String>) -> Result<BouncePlan> {
        let idle = self.inner.current_device().is_none_or(|d| d.player.idle());
        let threads = crate::export::export_threads(Threads::from_env()?, idle, false);
        self.call(move |a| {
            let o = a.open.as_ref().ok_or_else(|| anyhow!("no project is open"))?;
            if o.token != token {
                return Err(anyhow!("the open project changed; refresh the graph before editing"));
            }
            if let Some(why) = super::actor::nothing_to_render(&o.session) {
                bail!("{why}");
            }
            let rev = o.session.accepted();
            let plain = |e: m2_editor::EditError| anyhow!("{}", e.to_string().trim_start_matches("edit rejected: "));
            let bounce = Bounce::in_place_of(rev, &o.session.packages, &track).map_err(plain)?;
            let dir = rev.project.dir.clone();
            let asset = match name {
                Some(n) => {
                    let file = if n.ends_with(".wav") { n } else { format!("{n}.wav") };
                    AssetName::new(&file).map_err(|e| anyhow!("{e}: name it like `keys-bounce`"))?
                }
                None => free_name(&dir, &track),
            };
            let label = rev.compiled.ir.modules.iter().find(|m| m.key == track).map_or(track.clone(), |m| m.label().to_string());
            let render = bounce.render(&o.session.packages, threads).map_err(super::plainly)?;
            Ok(BouncePlan { bounce, render, asset, label, dir })
        })
    }

    /// Put `bounced`, the bounce of track `track`, in the open song (RENDER-40): one actor job, one
    /// undo step, "bouncing Keys" (or an agent's words). Answers the edit with the new track's
    /// `module` and `label`, the track `muted`, and the `asset`. Refused when the song changed
    /// since it was planned.
    pub fn bounce_place(&self, token: u64, track: String, bounced: Bounced) -> Result<Value> {
        let words = crate::caller::now().why();
        let done = self.call(move |a| {
            a.edit_and(token, |s| {
                let label =
                    s.accepted().compiled.ir.modules.iter().find(|m| m.key == track).map_or(track.clone(), |m| m.label().to_string());
                let p = m2_editor::bounce::in_place(s, &track, &bounced)?;
                s.name_step(p.edited.revision, &words.unwrap_or_else(|| format!("bouncing {label}")), true);
                Ok((p.edited, json!({"module": p.made.module, "label": p.made.label, "muted": track, "asset": bounced.asset.as_str()})))
            })
        })?;
        if let Some(k) = done["module"].as_str() {
            self.forget_overlays(&[k.to_string()]);
        }
        Ok(done)
    }
}
