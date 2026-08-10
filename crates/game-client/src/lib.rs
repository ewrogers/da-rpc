//! Integration boundary for the supported Dark Ages 7.41 game client.

mod runtime;
mod state;

pub use runtime::{
    ADVANCE_PATH_RVA, BUILD_PATH_RVA, CLIENT_MAIN_THREAD_ID_RVA, CLIENT_PACKET_SUBMIT_ENTRY,
    CLIENT_PACKET_SUBMIT_RVA, CLIENT_SOCKET_POINTER_RVA, EVENT_DISPATCH_ENTRY, EVENT_DISPATCH_RVA,
    EVENT_DISPATCHER_POINTER_RVA, EVENT_DISPATCHER_TICK_ENTRY, EVENT_DISPATCHER_TICK_RVA,
    GAME_MESSAGE_APPEND_RVA, GUI_BACK_PANE_GET_RVA, ITEM_ACTIVATE_RVA, MAP_SIZE_HANDLER_ENTRY,
    MAP_SIZE_HANDLER_RVA, NPC_SESSION_COL_RVAS, RESET_MOVEMENT_RVA, SELF_OBJECT_RVA,
    SKILL_ACTIVATE_RVA, SPELL_DELAY_ACTIVE_OFFSET, SPELL_DELAY_CONTROL_PANE_GET_RVA,
    SPELL_DELAY_CONTROL_PANE_POINTER_RVA, SPELL_DENIED_RVA, SPELL_NO_ARGS_RVA, SPELL_START_RVA,
    SPELL_TARGET_RVA, TURN_RVA, WALK_RVA, WORLD_ENTITY_INTERACTION_RVA, WORLD_PANE_ADJUSTMENT,
    WORLD_PANE_POINTER_RVA, WORLD_PANE_ROUTE_ACTIVE_OFFSET,
};
pub use state::{
    ABILITY_SLOT_COUNT, EFFECT_SLOT_COUNT, EQUIPMENT_SLOT_COUNT, GROUP_INVITATION_CAPACITY,
    GROUP_MEMBER_CAPACITY, GROUP_NAME_BYTES, INVENTORY_SLOT_COUNT, MAX_OBJECT_NAME_BYTES,
    MAX_WORLD_OBJECTS, MemoryReader, RawAppearance, RawCharacter, RawClientText, RawEffect,
    RawEffects, RawEquipment, RawEquipmentItem, RawGroupInvitation, RawGroupMember, RawGroupState,
    RawInventory, RawInventoryItem, RawLifecycle, RawLocation, RawMapName, RawModifiers,
    RawObjects, RawPaneProgression, RawSkill, RawSkillbook, RawSpell, RawSpellbook,
    RawStateSnapshot, RawWorldObject, StateReadError, StateWalker,
};

use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

pub const CLIENT_VERSION: &str = "7.41";
pub const CLIENT_VERSION_CODE: u32 = 741;
pub const WINDOW_CLASS: &str = "Darkages";
pub const EXECUTABLE_SIZE: u64 = 3_112_960;
pub const EXECUTABLE_SHA256: [u8; 32] = [
    0x05, 0x4A, 0x5D, 0x6A, 0xDC, 0x56, 0x09, 0x9C, 0x6B, 0xFD, 0x9D, 0x2A, 0x58, 0x67, 0x5A, 0xFF,
    0x62, 0xDC, 0x78, 0x8B, 0x63, 0x20, 0x9A, 0x3D, 0x90, 0x64, 0x92, 0xF5, 0xB8, 0x9E, 0x96, 0xC6,
];

#[cfg(debug_assertions)]
pub const DEBUG_UNSUPPORTED_CLIENT_BYPASS_ENVIRONMENT_VARIABLE: &str =
    "DARPC_LOADER_TEST_ALLOW_UNSUPPORTED_CLIENT";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchPatch {
    pub name: &'static str,
    pub rva: u32,
    pub expected: &'static [u8],
    pub replacement: &'static [u8],
}

pub const ALLOW_MULTIPLE_PATCHES: &[LaunchPatch] = &[LaunchPatch {
    name: "allow multiple clients",
    rva: 0x0017_A7D9,
    expected: &[0x75, 0x07],
    replacement: &[0xEB, 0x07],
}];

pub const COMMAND_LINE_ENDPOINT_PATCHES: &[LaunchPatch] = &[LaunchPatch {
    name: "command-line endpoint",
    rva: 0x0003_2253,
    expected: &[0xE8, 0x28, 0x11, 0x00, 0x00],
    replacement: &[0xE8, 0xB8, 0x0D, 0x00, 0x00],
}];

pub const DISABLE_ENDPOINT_FALLBACK_PATCHES: &[LaunchPatch] = &[LaunchPatch {
    name: "disable endpoint fallback",
    rva: 0x0016_55F4,
    expected: &[0xC7, 0x85, 0x94, 0xFB, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00],
    replacement: &[0xE9, 0x06, 0x13, 0x00, 0x00, 0x90, 0x90, 0x90, 0x90, 0x90],
}];

pub const SKIP_INTRO_PATCHES: &[LaunchPatch] = &[LaunchPatch {
    name: "skip intro",
    rva: 0x000A_CA85,
    expected: &[0x6A, 0x01],
    replacement: &[0x6A, 0x02],
}];

pub const SKIP_NOTICE_PATCHES: &[LaunchPatch] = &[
    LaunchPatch {
        name: "skip notice after cached greeting",
        rva: 0x000B_897C,
        expected: &[0x75, 0x6C],
        replacement: &[0xEB, 0x6C],
    },
    LaunchPatch {
        name: "skip notice after replacement greeting",
        rva: 0x000B_8ACF,
        expected: &[0x75, 0x6D],
        replacement: &[0xEB, 0x6D],
    },
    LaunchPatch {
        name: "enable early title menu input for skipped notice",
        rva: 0x000B_7BED,
        expected: &[0x74, 0x07],
        replacement: &[0xEB, 0x07],
    },
    LaunchPatch {
        name: "remove fixed server transfer delay for skipped notice",
        rva: 0x0016_4855,
        expected: &[0x68, 0xE8, 0x03, 0x00, 0x00],
        replacement: &[0x68, 0x00, 0x00, 0x00, 0x00],
    },
];

pub const CANCELLED_EXCHANGE_ALERT_PATCH: LaunchPatch = LaunchPatch {
    name: "skip cancelled exchange alert",
    rva: 0x0006_AA81,
    expected: &[
        0x6A, 0x00, 0x68, 0x34, 0x06, 0x00, 0x00, 0xE8, 0x43, 0x9A, 0x04, 0x00,
    ],
    replacement: &[
        0x31, 0xC0, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    ],
};

pub const COMPLETED_EXCHANGE_ALERT_PATCH: LaunchPatch = LaunchPatch {
    name: "skip completed exchange alert",
    rva: 0x0006_AC57,
    expected: &[
        0x6A, 0x00, 0x68, 0x34, 0x06, 0x00, 0x00, 0xE8, 0x6D, 0x98, 0x04, 0x00,
    ],
    replacement: &[
        0x31, 0xC0, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    ],
};

pub const SKIP_EXCHANGE_ALERTS_PATCHES: &[LaunchPatch] = &[
    CANCELLED_EXCHANGE_ALERT_PATCH,
    COMPLETED_EXCHANGE_ALERT_PATCH,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientExecutable {
    path: PathBuf,
}

impl ClientExecutable {
    pub fn validate(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let canonical_path = fs::canonicalize(path).map_err(|error| {
            format!(
                "failed to resolve client executable `{}`: {error}",
                path.display()
            )
        })?;
        let metadata = fs::metadata(&canonical_path).map_err(|error| {
            format!(
                "failed to inspect client executable `{}`: {error}",
                canonical_path.display()
            )
        })?;

        if !metadata.is_file() {
            return Err(format!(
                "client executable is not a file: `{}`",
                canonical_path.display()
            ));
        }

        if metadata.len() != EXECUTABLE_SIZE {
            return Err(format!(
                "unsupported client executable size: expected={EXECUTABLE_SIZE} actual={}",
                metadata.len()
            ));
        }

        let image = fs::read(&canonical_path).map_err(|error| {
            format!(
                "failed to read client executable `{}`: {error}",
                canonical_path.display()
            )
        })?;
        let actual_hash: [u8; 32] = Sha256::digest(&image).into();

        if actual_hash != EXECUTABLE_SHA256 {
            return Err(format!(
                "unsupported client executable SHA-256: expected={} actual={}",
                encode_hash(&EXECUTABLE_SHA256),
                encode_hash(&actual_hash)
            ));
        }

        Ok(Self {
            path: canonical_path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn executable_sha256() -> String {
    encode_hash(&EXECUTABLE_SHA256)
}

fn encode_hash(hash: &[u8; 32]) -> String {
    let mut encoded = String::with_capacity(hash.len() * 2);

    for byte in hash {
        write!(encoded, "{byte:02X}").expect("writing to a String cannot fail");
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        ALLOW_MULTIPLE_PATCHES, COMMAND_LINE_ENDPOINT_PATCHES, ClientExecutable,
        DISABLE_ENDPOINT_FALLBACK_PATCHES, EXECUTABLE_SHA256, EXECUTABLE_SIZE,
        SKIP_EXCHANGE_ALERTS_PATCHES, SKIP_INTRO_PATCHES, SKIP_NOTICE_PATCHES, executable_sha256,
    };
    use std::{fs, process};

    #[test]
    fn renders_the_supported_fingerprint() {
        assert_eq!(
            executable_sha256(),
            "054A5D6ADC56099C6BFD9D2A58675AFF62DC788B63209A3D906492F5B89E96C6"
        );
        assert_eq!(EXECUTABLE_SHA256.len(), 32);
    }

    #[test]
    fn launch_patch_contracts_are_complete_and_disjoint() {
        let patches = ALLOW_MULTIPLE_PATCHES
            .iter()
            .chain(COMMAND_LINE_ENDPOINT_PATCHES)
            .chain(DISABLE_ENDPOINT_FALLBACK_PATCHES)
            .chain(SKIP_INTRO_PATCHES)
            .chain(SKIP_NOTICE_PATCHES)
            .chain(SKIP_EXCHANGE_ALERTS_PATCHES)
            .collect::<Vec<_>>();

        assert_eq!(patches.len(), 10);

        for patch in &patches {
            assert!(!patch.expected.is_empty());
            assert_eq!(patch.expected.len(), patch.replacement.len());
        }

        for (index, patch) in patches.iter().enumerate() {
            let start = u64::from(patch.rva);
            let end = start + patch.expected.len() as u64;

            for other in patches.iter().skip(index + 1) {
                let other_start = u64::from(other.rva);
                let other_end = other_start + other.expected.len() as u64;
                assert!(end <= other_start || other_end <= start);
            }
        }
    }

    #[test]
    fn rejects_an_executable_with_the_wrong_size() {
        let path = std::env::temp_dir().join(format!(
            "darpc-game-client-wrong-size-{}.exe",
            process::id()
        ));
        fs::write(&path, b"not the supported client").expect("failed to write test executable");

        let error = ClientExecutable::validate(&path)
            .expect_err("wrong-sized executable unexpectedly validated");

        fs::remove_file(path).expect("failed to remove test executable");
        assert!(error.contains("unsupported client executable size"));
    }

    #[test]
    fn rejects_an_executable_with_the_wrong_hash() {
        let path = std::env::temp_dir().join(format!(
            "darpc-game-client-wrong-hash-{}.exe",
            process::id()
        ));
        let file = fs::File::create(&path).expect("failed to create test executable");
        file.set_len(EXECUTABLE_SIZE)
            .expect("failed to size test executable");

        let error = ClientExecutable::validate(&path)
            .expect_err("wrong executable fingerprint unexpectedly validated");

        fs::remove_file(path).expect("failed to remove test executable");
        assert!(error.contains("unsupported client executable SHA-256"));
    }
}
