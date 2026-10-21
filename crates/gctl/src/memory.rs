//! Write-only memory: values the mouse cannot read back, recorded after a
//! successful write. TOML under the state dir, keyed by model name and then by
//! profile for per-profile settings. The file stores the text forms; `load`
//! parses every value back through [`crate::parse`], so a file that does not
//! parse is an error, never silently emptied.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use gctl_proto::{Binding, Brightness, Button, Effect, MouseFn, Profile, Rgb, ScrollDirection};
use serde::{Deserialize, Serialize};

use crate::CmdError;
use crate::parse;

const FILE: &str = "memory.toml";

const NO_STATE_DIR: &str = "cannot find a state directory; set XDG_STATE_HOME or HOME";

/// The state dir for a command that loads or saves memory; fails when none
/// resolved, before any write.
pub(crate) fn require_state_dir(state_dir: Option<&Path>) -> Result<&Path, CmdError> {
    state_dir.ok_or_else(|| CmdError::Message(NO_STATE_DIR.to_owned()))
}

/// The typed, in-memory state. The scroll direction is never stored: it is
/// derived from the six remembered scroll binds.
#[derive(Debug, Clone, Default)]
pub(crate) struct Memory {
    model: BTreeMap<String, ModelMem>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ModelMem {
    pub(crate) brightness: Option<Brightness>,
    profile: HashMap<Profile, ProfileMem>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProfileMem {
    pub(crate) effect: Option<Effect>,
    pub(crate) dpi_colors: Option<Vec<Rgb>>,
    pub(crate) binds: BTreeMap<String, Binding>,
}

/// The on-disk representation: text forms only, so the schema stays readable.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireMemory {
    #[serde(default)]
    model: BTreeMap<String, WireModel>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireModel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brightness: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    profile: BTreeMap<String, WireProfile>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effect: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dpi_colors: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    binds: BTreeMap<String, String>,
}

impl Memory {
    /// Reads the memory fresh from disk. A missing file is empty; a file that
    /// cannot be read or that does not parse is a refusal naming the file.
    pub(crate) fn load(state_dir: Option<&Path>) -> Result<Self, CmdError> {
        let state_dir = require_state_dir(state_dir)?;
        let path = state_dir.join(FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(read_error(&path, &error.to_string())),
        };
        let wire: WireMemory = toml::from_str(&text)
            .map_err(|error| read_error(&path, &crate::toml_error(&text, &error)))?;
        Self::from_wire(wire).map_err(|cause| read_error(&path, &cause))
    }

    /// Writes the memory through a temp file in the state dir, then renames it
    /// over the real file. The state dir is created first.
    pub(crate) fn save(&self, state_dir: &Path) -> io::Result<()> {
        let text = toml::to_string(&self.to_wire()).map_err(io::Error::other)?;
        fs::create_dir_all(state_dir)?;
        let path = state_dir.join(FILE);
        let tmp = state_dir.join(format!(".{FILE}.{}.tmp", std::process::id()));
        let mut file = fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        if let Err(error) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        Ok(())
    }

    fn to_wire(&self) -> WireMemory {
        let model = self
            .model
            .iter()
            .map(|(name, mem)| {
                let profile = mem
                    .profile
                    .iter()
                    .map(|(profile, mem)| {
                        let binds = mem
                            .binds
                            .iter()
                            .map(|(button, binding)| {
                                (button.clone(), parse::format_binding(*binding))
                            })
                            .collect();
                        (
                            parse::format_profile(*profile),
                            WireProfile {
                                effect: mem.effect.as_ref().map(parse::format_effect),
                                dpi_colors: mem.dpi_colors.as_deref().map(parse::format_colors),
                                binds,
                            },
                        )
                    })
                    .collect();
                (
                    name.clone(),
                    WireModel {
                        brightness: mem.brightness.map(parse::format_brightness),
                        profile,
                    },
                )
            })
            .collect();
        WireMemory { model }
    }

    fn from_wire(wire: WireMemory) -> Result<Self, String> {
        let mut model = BTreeMap::new();
        for (name, wire_model) in wire.model {
            let brightness = wire_model
                .brightness
                .as_deref()
                .map(parse::brightness)
                .transpose()
                .map_err(|error| error.0)?;
            let mut profile = HashMap::new();
            for (id, wire_profile) in wire_model.profile {
                let profile_id = parse::profile(&id).map_err(|error| error.0)?;
                let effect = wire_profile
                    .effect
                    .as_deref()
                    .map(parse::effect)
                    .transpose()
                    .map_err(|error| error.0)?;
                let dpi_colors = wire_profile
                    .dpi_colors
                    .as_deref()
                    .map(|text| parse::colors(text.split_whitespace()))
                    .transpose()
                    .map_err(|error| error.0)?;
                let mut binds = BTreeMap::new();
                for (button, binding) in wire_profile.binds {
                    let button = parse::button(&button).map_err(|error| error.0)?;
                    let binding = parse::binding(&binding).map_err(|error| error.0)?;
                    binds.insert(parse::format_button(button).to_owned(), binding);
                }
                profile.insert(
                    profile_id,
                    ProfileMem {
                        effect,
                        dpi_colors,
                        binds,
                    },
                );
            }
            model.insert(
                name,
                ModelMem {
                    brightness,
                    profile,
                },
            );
        }
        Ok(Self { model })
    }

    pub(crate) fn model(&mut self, name: &str) -> &mut ModelMem {
        self.model.entry(name.to_owned()).or_default()
    }

    pub(crate) fn profile(&mut self, name: &str, profile: Profile) -> &mut ProfileMem {
        self.model(name).profile.entry(profile).or_default()
    }

    pub(crate) fn brightness(&self, name: &str) -> Option<Brightness> {
        self.model.get(name).and_then(|mem| mem.brightness)
    }

    pub(crate) fn effect(&self, name: &str, profile: Profile) -> Option<&Effect> {
        self.profile_mem(name, profile)
            .and_then(|mem| mem.effect.as_ref())
    }

    pub(crate) fn dpi_colors(&self, name: &str, profile: Profile) -> Option<&[Rgb]> {
        self.profile_mem(name, profile)
            .and_then(|mem| mem.dpi_colors.as_deref())
    }

    pub(crate) fn bind(&self, name: &str, profile: Profile, button: Button) -> Option<Binding> {
        self.profile_mem(name, profile)
            .and_then(|mem| mem.binds.get(parse::format_button(button)).copied())
    }

    pub(crate) fn binds(&self, name: &str, profile: Profile) -> Option<&BTreeMap<String, Binding>> {
        self.profile_mem(name, profile).map(|mem| &mem.binds)
    }

    /// The scroll direction, derived from the six remembered scroll binds:
    /// `invert` or `default` when all six match that direction, else unknown.
    pub(crate) fn scroll(&self, name: &str) -> Option<ScrollDirection> {
        let model = self.model.get(name)?;
        let mut direction: Option<ScrollDirection> = None;
        for profile in Profile::all() {
            let mem = model.profile.get(&profile)?;
            let matched = match (mem.binds.get("scroll-up"), mem.binds.get("scroll-down")) {
                (
                    Some(Binding::Mouse(MouseFn::ScrollDown)),
                    Some(Binding::Mouse(MouseFn::ScrollUp)),
                ) => ScrollDirection::Invert,
                (
                    Some(Binding::Mouse(MouseFn::ScrollUp)),
                    Some(Binding::Mouse(MouseFn::ScrollDown)),
                ) => ScrollDirection::Default,
                _ => return None,
            };
            match direction {
                None => direction = Some(matched),
                Some(existing) if existing == matched => {}
                Some(_) => return None,
            }
        }
        direction
    }

    fn profile_mem(&self, name: &str, profile: Profile) -> Option<&ProfileMem> {
        self.model
            .get(name)
            .and_then(|mem| mem.profile.get(&profile))
    }
}

fn read_error(path: &Path, cause: &str) -> CmdError {
    CmdError::Message(format!(
        "cannot read remembered values from {path:?}: {cause}; fix or delete the file"
    ))
}

/// The save error message, naming the memory file. `mouse_written` is true when
/// the device write already succeeded, so the mouse holds the new value.
pub(crate) fn save_message(state_dir: &Path, error: &io::Error, mouse_written: bool) -> String {
    let path = state_dir.join(FILE);
    let mut message = format!("could not save remembered values to {path:?}: {error}");
    if mouse_written {
        message.push_str(" (the mouse was written)");
    }
    message
}
