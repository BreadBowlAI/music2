//! MIDI files and the open song (studio lane C): the song or one clip as a MIDI file's bytes
//! ([`m2_editor::midifile`]), made on the actor from the accepted revision, as every view is. The
//! file is written by the route, off the actor.

use anyhow::{Result, anyhow};
use m2_editor::midifile::ClipAt;

use super::Graph;

impl Graph {
    /// The open song as a MIDI file, or `clip` alone, and the name its file takes: the song's
    /// ("first-song"), or the song's and the clip's track's ("first-song Bass"). Reads only.
    pub fn midi_file(&self, token: u64, clip: Option<ClipAt>) -> Result<(Vec<u8>, String)> {
        self.call(move |a| {
            let o = a.open.as_ref().ok_or_else(|| anyhow!("no project is open"))?;
            if o.token != token {
                return Err(anyhow!("the open project changed; refresh the graph before editing"));
            }
            let song = o.session.accepted().project.manifest.project.name.clone();
            let plain = |e: m2_editor::EditError| anyhow!("{}", e.to_string().trim_start_matches("edit rejected: "));
            match clip {
                Some(c) => {
                    let (bytes, track) = m2_editor::midifile::export_clip(&o.session, &c.module, &c.lane, &c.id).map_err(plain)?;
                    Ok((bytes, format!("{song} {track}")))
                }
                None => Ok((m2_editor::midifile::export_song(&o.session).map_err(plain)?, song)),
            }
        })
    }
}
