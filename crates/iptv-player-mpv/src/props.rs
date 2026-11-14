//! The native component wire contract, independent of either host toolkit.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceProps {
    pub url: String,
    pub public_only: bool,
    pub headers: BTreeMap<String, String>,
    pub token: u64,
    pub live: bool,
    pub reconnect: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PipProps {
    pub auto: bool,
    pub enter: u64,
    pub window: bool,
}

// One declaration keeps the shell's typed reactive bindings and the serialized
// snapshot contract in sync, without coupling the adapter to UI signals.
macro_rules! player_props {
    ($($field:ident: $ty:ty,)*) => {
        #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct PlayerProps {
            $(pub $field: $ty,)*
        }

        /// Independently evaluated properties: changing sound does not read a source.
        pub struct PlayerBindings {
            $(pub $field: Box<dyn Fn() -> $ty>,)*
        }
    };
}

player_props! {
    source: Option<SourceProps>,
    paused: bool,
    picture_size: String,
    video_track: String,
    audio_track: String,
    text_track: String,
    volume: u8,
    muted: bool,
    audio_delay: i32,
    pip: Option<PipProps>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_android_fixture_is_the_exact_rust_wire_format() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/player-props.json")).unwrap();
        let props: PlayerProps = serde_json::from_value(fixture.clone()).unwrap();
        assert_eq!(props.source.as_ref().unwrap().token, 37);
        assert_eq!(
            props.source.as_ref().unwrap().headers["X-Synthetic"],
            "fixture"
        );
        assert_eq!(serde_json::to_value(props).unwrap(), fixture);
    }
}
