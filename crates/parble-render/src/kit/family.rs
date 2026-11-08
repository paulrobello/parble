//! Runtime texture family (ported from hex-world `hex-assets::texture_family`): BC7 KTX2 where the
//! device has BC compression, else ASTC KTX2, else PNG. The material set ships all three
//! (`assets/materials/<set>/<set>.<map>.{bc7.ktx2,astc.ktx2,png}`).

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TextureFamily {
    Bc7,
    Astc,
    Png,
}

impl TextureFamily {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "bc7" => Some(Self::Bc7),
            "astc" => Some(Self::Astc),
            "png" => Some(Self::Png),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Bc7 => "bc7",
            Self::Astc => "astc",
            Self::Png => "png",
        }
    }

    /// File suffix after `<set>.<map>.`.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Bc7 => "bc7.ktx2",
            Self::Astc => "astc.ktx2",
            Self::Png => "png",
        }
    }
}

/// Picks the family from the device's compression features (PRD §9.3).
pub fn select_family(bc: bool, astc: bool) -> TextureFamily {
    if bc {
        TextureFamily::Bc7
    } else if astc {
        TextureFamily::Astc
    } else {
        TextureFamily::Png
    }
}

/// [`select_family`] capped by an agent override (`--tex`): the override can only step down the
/// BC7 → ASTC → PNG order, never pick a family the device cannot sample.
pub fn select_family_capped(bc: bool, astc: bool, cap: Option<TextureFamily>) -> TextureFamily {
    match cap {
        None | Some(TextureFamily::Bc7) => select_family(bc, astc),
        Some(TextureFamily::Astc) => select_family(false, astc),
        Some(TextureFamily::Png) => TextureFamily::Png,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_bc7_on_desktop() {
        assert_eq!(select_family(true, false), TextureFamily::Bc7);
        assert_eq!(select_family(true, true), TextureFamily::Bc7);
    }

    #[test]
    fn falls_back_to_astc_then_png() {
        assert_eq!(select_family(false, true), TextureFamily::Astc);
        assert_eq!(select_family(false, false), TextureFamily::Png);
    }

    #[test]
    fn suffixes_and_names_round_trip() {
        for f in [TextureFamily::Bc7, TextureFamily::Astc, TextureFamily::Png] {
            assert_eq!(TextureFamily::parse(f.name()), Some(f));
        }
        assert_eq!(TextureFamily::Bc7.suffix(), "bc7.ktx2");
        assert_eq!(TextureFamily::Astc.suffix(), "astc.ktx2");
        assert_eq!(TextureFamily::Png.suffix(), "png");
    }

    #[test]
    fn override_only_steps_down() {
        use TextureFamily::*;
        assert_eq!(select_family_capped(true, true, None), Bc7);
        assert_eq!(select_family_capped(true, true, Some(Astc)), Astc);
        assert_eq!(select_family_capped(true, false, Some(Astc)), Png);
        assert_eq!(select_family_capped(true, true, Some(Png)), Png);
        assert_eq!(select_family_capped(false, true, Some(Bc7)), Astc);
        assert_eq!(select_family_capped(false, false, Some(Bc7)), Png);
    }

    /// Every family file the kit material loads exists in the repo, so a served build can fetch it.
    #[test]
    fn every_family_file_ships() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/");
        for f in [TextureFamily::Bc7, TextureFamily::Astc, TextureFamily::Png] {
            for set in super::super::WOOD_SETS {
                for map in super::super::WOOD_MAPS {
                    let p = format!("{root}{}", super::super::wood_path(set, map, f));
                    assert!(std::path::Path::new(&p).is_file(), "missing {p}");
                }
            }
        }
    }
}
