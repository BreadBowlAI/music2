//! The editable source subset. A literal lens can read a numeric token and propose
//! its replacement. It cannot choose a graph target or approve its own result:
//! revision resolution and independent graph verification belong to the session.

use m2_ir::Origin;
use m2_script::sites::{Arg, Site};

use super::EditError;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LiteralLens {
    pub start: u32,
    pub end: u32,
    pub literal: String,
    pub scale: f64,
}

pub(crate) struct LiteralEdit {
    pub text: String,
    /// What the written token evaluates to, in canonical units. Both preview and
    /// commit use this value, including rounding through the source unit conversion.
    pub value: f64,
}

impl LiteralLens {
    pub fn read(site: &Site, origin: &Origin, source: &str, value: f64) -> Option<Self> {
        let Arg::Number { start, end, value: written } = site.arg(&origin.arg)? else { return None };
        let scale = origin.scale.unwrap_or(1.0);
        if !scale.is_finite() || scale <= 0.0 || !value.is_finite() || written * scale != value {
            return None;
        }
        Some(Self { start: *start, end: *end, literal: source.get(*start as usize..*end as usize)?.into(), scale })
    }

    pub fn propose(&self, value: f64) -> Result<LiteralEdit, EditError> {
        let written = value / self.scale;
        if !value.is_finite() || !written.is_finite() {
            return Err(EditError::Rejected("the control needs a finite number".into()));
        }
        let text = if written == 0.0 { "0".into() } else { written.to_string() };
        let value = text.parse::<f64>().expect("formatted number") * self.scale;
        if !value.is_finite() || !(value as f32).is_finite() {
            return Err(EditError::Rejected("the value is outside the audio engine's numeric range".into()));
        }
        Ok(LiteralEdit { text, value })
    }
}
