//! F3R (Pokemon Reloaded) compatibility manifest.
//!
//! Every native room, object, script, global, instance field, sprite and save-path
//! identifier used by the emulator is declared here. Core code addresses content
//! through these content keys and constants; it must not carry native names or
//! numeric ids of its own.
//!
//! Numeric ids are build-scoped. A different F3R build needs a new manifest
//! revision rather than core changes.

/// Manifest revision. Increment when native ids, names, or the supported build
/// family change.
pub const MANIFEST_VERSION: u32 = 3;

/// Native build family this manifest was written for.
pub const NATIVE_BUILD_FAMILY: &str = "Pokemon Reloaded The Last Beta 1.9.2.2";
/// Build version stamp observed for the supported family.
pub const NATIVE_BUILD_STAMP: &str = "20260802";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuildInfo {
    pub manifest: u32,
    pub family: &'static str,
    pub stamp: &'static str,
    pub rooms: u32,
    pub objects: u32,
    pub scripts: u32,
    pub globals: u32,
    pub save_slots: u32,
}

pub const BUILD: BuildInfo = BuildInfo {
    manifest: MANIFEST_VERSION,
    family: NATIVE_BUILD_FAMILY,
    stamp: NATIVE_BUILD_STAMP,
    rooms: 717,
    objects: 1991,
    scripts: 5075,
    globals: 523,
    save_slots: 99,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Kind {
    Room,
    Object,
    Script,
    Global,
    Field,
    Sprite,
    SavePath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Supported,
    Unsupported,
    DisplayOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Entry {
    pub kind: Kind,
    pub native_id: Option<i32>,
    pub native_name: &'static str,
    pub content_key: &'static str,
    pub aliases: &'static [&'static str],
    pub role: Option<&'static str>,
    pub status: Status,
}

const fn room(id: i32, name: &'static str, key: &'static str, role: &'static str) -> Entry {
    Entry {
        kind: Kind::Room,
        native_id: Some(id),
        native_name: name,
        content_key: key,
        aliases: &[],
        role: Some(role),
        status: Status::Supported,
    }
}

const fn object(id: Option<i32>, name: &'static str, key: &'static str, role: &'static str) -> Entry {
    object_aliased(id, name, key, role, &[])
}

const fn object_aliased(
    id: Option<i32>,
    name: &'static str,
    key: &'static str,
    role: &'static str,
    aliases: &'static [&'static str],
) -> Entry {
    Entry {
        kind: Kind::Object,
        native_id: id,
        native_name: name,
        content_key: key,
        aliases,
        role: Some(role),
        status: Status::Supported,
    }
}

const fn script_status(
    id: Option<i32>,
    name: &'static str,
    key: &'static str,
    role: &'static str,
    status: Status,
) -> Entry {
    Entry {
        kind: Kind::Script,
        native_id: id,
        native_name: name,
        content_key: key,
        aliases: &[],
        role: Some(role),
        status,
    }
}

const fn script(id: Option<i32>, name: &'static str, key: &'static str, role: &'static str) -> Entry {
    script_status(id, name, key, role, Status::Supported)
}

const fn global(name: &'static str, key: &'static str, role: &'static str) -> Entry {
    Entry {
        kind: Kind::Global,
        native_id: None,
        native_name: name,
        content_key: key,
        aliases: &[],
        role: Some(role),
        status: Status::Supported,
    }
}

const fn field(name: &'static str, key: &'static str, role: &'static str) -> Entry {
    Entry {
        kind: Kind::Field,
        native_id: None,
        native_name: name,
        content_key: key,
        aliases: &[],
        role: Some(role),
        status: Status::Supported,
    }
}

const fn sprite(id: i32, name: &'static str, key: &'static str, role: &'static str) -> Entry {
    Entry {
        kind: Kind::Sprite,
        native_id: Some(id),
        native_name: name,
        content_key: key,
        aliases: &[],
        role: Some(role),
        status: Status::Supported,
    }
}

const fn save_path(name: &'static str, key: &'static str, role: &'static str) -> Entry {
    Entry {
        kind: Kind::SavePath,
        native_id: None,
        native_name: name,
        content_key: key,
        aliases: &[],
        role: Some(role),
        status: Status::Supported,
    }
}

pub mod room {
    //! Room content keys and the native ids they stand for.

    pub const CENTER: i32 = 2;
    pub const SELECT: i32 = 3;
    pub const HOME_1: i32 = 9;
    pub const HOME_2: i32 = 10;
    pub const EVOLUTION: i32 = 12;
    pub const MAP: i32 = 21;
    pub const TITLE: i32 = 22;
    pub const BOOT: i32 = 30;
    pub const LOADER: i32 = 43;
    pub const BATTLE_AQUA: i32 = 47;
    pub const BATTLE: i32 = 191;
    pub const PUZZLE: i32 = 202;
    pub const MULTIPLAYER_MENU: i32 = 461;
    pub const OPTIONS: i32 = 465;
    pub const CREDITS: i32 = 466;
    pub const LAB_FOLLOWUP: i32 = 527;
    pub const LAB_INTRO: i32 = 528;
    pub const BATTLE_LAVA: i32 = 709;
    pub const SAFARI: i32 = 715;

    const NO_PLAYER_PREFIXES: [&str; 3] = ["inicio", "select", "intro"];
    const CHARLA_PREFIX: &str = "charla";
    const MENU_FRAGMENTS: [&str; 10] = [
        "menu", "inicio", "loader", "option", "opcion", "credito", "copy", "splash", "intro",
        "select",
    ];
    const BAG_FRAGMENTS: [&str; 2] = ["mochila", "bagroom"];
    const OPTIONS_FRAGMENTS: [&str; 2] = ["opcion", "option"];

    pub fn is_no_player_name(name_lower: &str) -> bool {
        NO_PLAYER_PREFIXES.iter().any(|prefix| name_lower.starts_with(prefix))
    }

    pub fn is_charla_name(name_lower: &str) -> bool {
        name_lower.starts_with(CHARLA_PREFIX)
    }

    pub fn is_menu_name(name_lower: &str) -> bool {
        MENU_FRAGMENTS.iter().any(|fragment| name_lower.contains(fragment))
    }

    pub fn is_bag_name(name_lower: &str) -> bool {
        BAG_FRAGMENTS.iter().any(|fragment| name_lower.contains(fragment))
    }

    pub fn is_options_name(name_lower: &str) -> bool {
        OPTIONS_FRAGMENTS.iter().any(|fragment| name_lower.contains(fragment))
    }
}

pub mod object {
    //! Object content keys. Names are kept as bytes because the engine compares
    //! native asset names byte-for-byte.

    pub const POKEMON_PLAYER: i32 = 0;
    pub const CONTROL: i32 = 3;
    pub const POKEMON_ENEMY: i32 = 4;
    pub const TRAINER_PLAYER: i32 = 7;
    pub const TRAINER_ENEMY: i32 = 9;
    pub const PLAYER: i32 = 14;
    pub const VARIABLES: i32 = 18;
    pub const EXCLAMATION: i32 = 95;
    pub const INICIO_CONTROL: i32 = 98;
    pub const MULTIPLAYER: i32 = 1172;
    /// Legacy alias for the multiplayer controller object.
    pub const PLAYER_TWO: i32 = MULTIPLAYER;

    pub const PLAYER_NAME: &[u8] = b"obj_Prota";
    pub const INICIO_CONTROL_NAME: &[u8] = b"InicioControl";
    pub const MULTIPLAYER_NAME: &[u8] = b"MULTIPLAYER";
    pub const EXCLAMATION_NAME: &[u8] = b"obj_EXC";

    pub fn is_player_name(name: &[u8]) -> bool {
        name == PLAYER_NAME
    }

    pub fn is_inicio_control_name(name: &[u8]) -> bool {
        name == INICIO_CONTROL_NAME
    }

    pub fn is_multiplayer_name(name: &[u8]) -> bool {
        name == MULTIPLAYER_NAME
    }

    pub fn is_exclamation_name(name: &[u8]) -> bool {
        name == EXCLAMATION_NAME
    }
}

pub mod sprite {
    //! Sprite content keys and native names.

    pub const BALL: i32 = 63;
    pub const PLAYER_DEFAULT: i32 = super::player::DEFAULT_SPRITE;

    pub const BALL_NAME: &[u8] = b"spr_ball";
    pub const PLAYER_DEFAULT_NAME: &[u8] = b"Mark2_D";
}

pub mod script {
    //! Script content keys and native names.

    pub const ADJUSTMENTS_ROOM: i32 = 5153;
    pub const CARGANDO_EQUIPO: i32 = 5603;
    pub const EXECUTE_FILE: i32 = 7575;
    pub const FILE_WRITE_DATA: i32 = 7639;
    pub const GET_RELOADED_VERSION: i32 = 7810;
    pub const GUARDANDO_EQUIPO: i32 = 7942;
    pub const GUARDAR_PARTIDA: i32 = 7946;
    pub const LOAD_CUSTOM_CHARACTERS: i32 = 8443;
    pub const LOADER_CONTROL_CREATE_SLOTS: i32 = 8445;
    pub const LOAD_FILE_INFO: i32 = 8453;
    pub const LOAD_FILE_SAV: i32 = 8454;
    pub const PRE_SAVE_AC: i32 = 9222;
    pub const PRE_SAVE_LINE: i32 = 9224;
    pub const SAVE_FILE_INFO: i32 = 9506;
    pub const SAVE_FILE_SAV: i32 = 9507;
    pub const SLOT_BOX_CARGAR: i32 = 9705;
    pub const SLOT_BOX_DELETE: i32 = 9706;
    pub const SLOT_BOX_RESTORE: i32 = 9708;

    pub const MODAL_DIALOG_PORTRAIT: i32 = 6937;
    pub const MODAL_DIALOG_MOBILE: i32 = 6941;
    pub const MODAL_DIALOG_PLAIN: i32 = 6949;
    pub const MODAL_PREFERENCES: i32 = 7744;
    pub const MODAL_TEXT_ENTRY: i32 = 7754;
    pub const MODAL_LIST: i32 = 7778;
    pub const MODAL_DIFFICULTY: i32 = 7836;
    pub const MODAL_QUESTION: i32 = 9673;
    pub const MODAL_QUESTION_LARGE: i32 = 9674;

    pub const DAY_NIGHT_MANAGE_CIELO: &[u8] = b"DayNightManageCielo";
    pub const DRAW_OUTDOOR_LIGHTING: &[u8] = b"DrawOutdoorLighting";
    pub const INITIALIZE_CIELO: &[u8] = b"InitializeCielo";
    pub const MULTIPLAYER_IMPORT_PLAYER_ONE: &[u8] = b"MultiplayerImportPlayerOne";
    pub const MULTIPLAYER_IMPORT_PLAYER_TWO: &[u8] = b"MultiplayerImportPlayerTwo";
    pub const MULTIPLAYER_GO_BATTLE: &[u8] = b"MultiplayerGoBattle";
    pub const VALOR_TRAINER_TRUCADO: &[u8] = b"ValorTrainerTrucado";
    pub const SPRITE_TRAINER: &[u8] = b"SpriteTrainer";
    pub const GET_SECURITY_PK: &[u8] = b"GetSecurityPK";
    pub const GET_SECURITY_PK_2: &[u8] = b"GetSecurityPK2";
    pub const SECURITY_PK: &[u8] = b"SecurityPK";
    pub const SECURITY_PK_2: &[u8] = b"SecurityPK2";
    pub const NET_TEAM_SPRITES: &[u8] = b"NetTeamSprites";
    pub const GUARDAR_PARTIDA_NAME: &[u8] = b"GuardarPartida";

    pub fn opens_dialogue(modal: i32) -> bool {
        matches!(modal, MODAL_DIALOG_PORTRAIT | MODAL_DIALOG_PLAIN | MODAL_DIALOG_MOBILE)
    }

    pub fn is_text_entry(modal: i32) -> bool {
        modal == MODAL_TEXT_ENTRY
    }

    pub fn is_gb_skipped(name: &[u8]) -> bool {
        [DAY_NIGHT_MANAGE_CIELO, DRAW_OUTDOOR_LIGHTING, INITIALIZE_CIELO].contains(&name)
    }
}

pub mod global {
    //! Global content keys. All names are raw bytes; the engine's global table is
    //! byte-keyed.

    pub const ACTUAL1: &[u8] = b"actual1";
    pub const ACTUAL2: &[u8] = b"actual2";
    pub const APODO_NAME: &[u8] = b"apodoName";
    pub const ATTACK1: &[u8] = b"attack1";
    pub const ATTACK2: &[u8] = b"attack2";
    pub const BATTLE: &[u8] = b"battle";
    pub const BATTLE_TURNOS: &[u8] = b"battleTurnos";
    pub const DRAW_MODAL_SYSTEM: &[u8] = b"drawModalSystem";
    pub const ENCAJA: &[u8] = b"ENCAJA";
    pub const ENEMIGOTRAINER: &[u8] = b"enemigotrainer";
    pub const ENEMIGOTRAINER2: &[u8] = b"enemigotrainer2";
    pub const ENEMIGOTRAINER3: &[u8] = b"enemigotrainer3";
    pub const ENTR_SPR: &[u8] = b"ENTR_SPR";
    pub const EXP1: &[u8] = b"exp1";
    pub const EXP2: &[u8] = b"exp2";
    pub const EXTRA_INFO: &[u8] = b"extraInfo";
    pub const HIERBA: &[u8] = b"hierba";
    pub const INDEXNIVEL: &[u8] = b"indexnivel";
    pub const INDEXPKMN: &[u8] = b"indexpkmn";
    pub const INFO: &[u8] = b"info";
    pub const INFO2: &[u8] = b"info2";
    pub const ISEP2: &[u8] = b"ISEP2";
    pub const IV: &[u8] = b"IV";
    pub const IV2: &[u8] = b"IV2";
    pub const MAX_PP_1: &[u8] = b"MAX_PP_1";
    pub const MAX_PS_1: &[u8] = b"MAX_PS_1";
    pub const MAX_PS_2: &[u8] = b"MAX_PS_2";
    pub const XTRA_PS_1: &[u8] = b"XTRA_PS_1";
    pub const ATK1: &[u8] = b"atk1";
    pub const DEF1: &[u8] = b"def1";
    pub const VEL1: &[u8] = b"vel1";
    pub const ATK_SP1: &[u8] = b"atk_sp1";
    pub const DEF_SP1: &[u8] = b"def_sp1";
    pub const PREC1: &[u8] = b"prec1";
    pub const NUM_ATTACK1: &[u8] = b"num_attack1";
    pub const TIPO_A1: &[u8] = b"tipoA1";
    pub const TIPO_B1: &[u8] = b"tipoB1";
    pub const TIPO_C1: &[u8] = b"tipoC1";
    pub const VOLADOR1: &[u8] = b"volador1";
    pub const FANTASMA1: &[u8] = b"fantasma1";
    pub const MODE_BATTLE_BACK: &[u8] = b"modeBattleBack";
    pub const NET_DELAY: &[u8] = b"NET_DELAY";
    pub const NETW_BROOM: &[u8] = b"NETW_BROOM";
    pub const NIVEL1: &[u8] = b"nivel1";
    pub const NIVEL2: &[u8] = b"nivel2";
    pub const NUMFONDOBATTLE: &[u8] = b"numfondobattle";
    pub const OBJPOKE1: &[u8] = b"objpoke1";
    pub const OBJPOKE2: &[u8] = b"objpoke2";
    pub const PKMN1: &[u8] = b"pkmn1";
    pub const PKMN2: &[u8] = b"pkmn2";
    pub const PKMNSEX1: &[u8] = b"pkmnsex1";
    pub const PKMNSEX2: &[u8] = b"pkmnsex2";
    pub const PLAYER2: &[u8] = b"PLAYER2";
    pub const PP_1: &[u8] = b"PP_1";
    pub const PP_2: &[u8] = b"PP_2";
    pub const PROTA_ID: &[u8] = b"prota_ID";
    pub const PROTA_INDEX: &[u8] = b"prota_index";
    pub const PROTA_VESTUARIO: &[u8] = b"prota_vestuario";
    pub const PROTANAME: &[u8] = b"protaname";
    pub const PS_1: &[u8] = b"PS_1";
    pub const PS_2: &[u8] = b"PS_2";
    pub const SELUE_PK1: &[u8] = b"selue_pk1";
    pub const SELUE_PK2: &[u8] = b"selue_pk2";
    pub const SLOT: &[u8] = b"Slot";
    pub const SPRD: &[u8] = b"sprD";
    pub const STD1: &[u8] = b"STD1";
    pub const STD2: &[u8] = b"STD2";
    pub const TRAINER_NM: &[u8] = b"TRAINER_NM";
    pub const TRAINER_SPR: &[u8] = b"TRAINER_SPR";
}

pub mod field {
    //! Instance-field content keys used by the engine bridges.

    pub const BTTBACK_INDEX: &[u8] = b"BTTBACK_INDEX";
    pub const CHAR2: &[u8] = b"char2";
    pub const FRASE2: &[u8] = b"frase2";
    pub const FRASE_PROPIA: &[u8] = b"frasePropia";
    pub const ID2: &[u8] = b"id2";
    pub const INDEX2: &[u8] = b"index2";
    pub const MASK1: &[u8] = b"mask1";
    pub const NUM_POKEMON1: &[u8] = b"NUM_POKEMON1";
    pub const NUM_POKEMON2: &[u8] = b"NUM_POKEMON2";
    pub const P2_SPRR: &[u8] = b"p2_sprR";
    pub const PKMN1: &[u8] = b"PKMN1";
    pub const PKMN2: &[u8] = b"PKMN2";
    pub const PLAYER1_INI: &[u8] = b"PLAYER1_INI";
    pub const PLAYER2_INI: &[u8] = b"PLAYER2_INI";
    pub const PROTANAME2: &[u8] = b"protaname2";
    pub const SEL: &[u8] = b"SEL";
    pub const SP_CHAR2: &[u8] = b"sp_char2";
    pub const SPRINDEX: &[u8] = b"sprINDEX";
    pub const SPRINDEX2: &[u8] = b"sprINDEX2";
    pub const SPR_R: &[u8] = b"sprR";
}

pub mod battle {
    //! Battle content keys and the special values the engine must reproduce.

    /// Battle sprite slot used when the companion faces zero.
    pub const SLOT_FACING_ZERO: i32 = 10;
    /// Battle sprite slot used by the hurt/back pose.
    pub const SLOT_FACING_HURT: i32 = 11;
    /// Battle sprite slot used when the game reports no usable facing.
    pub const SLOT_DEFAULT: i32 = 2;

    /// `global.hierba` value that selects the lava battle room.
    pub const GRASS_LAVA: i32 = 5;
    /// Number of battle backgrounds the game rolls when none is set.
    pub const BACKGROUND_COUNT: u32 = 253;
    /// Trainer name the game writes for a wild encounter.
    pub const WILD_TRAINER_NAME: &str = "salvaje";

    pub fn is_wild_trainer_name(name: &str) -> bool {
        name.eq_ignore_ascii_case(WILD_TRAINER_NAME)
    }
}

pub mod player {
    //! Player index and sprite conventions.

    /// `prota_index` value for a custom character.
    pub const CUSTOM_INDEX: i32 = 10;
    /// Base sprite index for roster characters (`prota_index + offset`).
    pub const SPRITE_OFFSET: i32 = 1000;
    /// Sprite used when the player instance and `sprD` cannot be read.
    pub const DEFAULT_SPRITE: i32 = 1860;

    pub fn is_custom(index: i32) -> bool {
        index == CUSTOM_INDEX
    }

    pub fn sprite_index(index: i32, custom_sprite: i32) -> i32 {
        if is_custom(index) {
            custom_sprite
        } else {
            index + SPRITE_OFFSET
        }
    }
}

pub mod format {
    //! Sizes, strides and field positions of the native party/trade layout.

    pub const PARTY_SIZE: usize = 6;
    pub const MOVE_COUNT: usize = 4;
    pub const INFO_FIELDS: usize = 17;
    pub const TEAM_INFO_LINES: usize = 16;
    pub const PARTY_ARRAY_STRIDE: u32 = 32_000;

    pub const PARTY_SCALAR_COUNT: usize = 28;
    pub const FIELD_COUNT: usize = 49;

    pub const SPECIES_FIELD: usize = 0;
    pub const LEVEL_FIELD: usize = 1;
    pub const INDEX_SPECIES_FIELD: usize = 7;
    pub const SELUE_SCALAR_FIELD: usize = 9;
    pub const OT_INFO_FIELD: usize = 4;
    pub const NICKNAME_INFO_FIELD: usize = 5;
    pub const SELUE_INFO_FIELD: usize = 14;
}

pub mod trade {
    //! Wire format shared by the host adapter and the trade codec.

    pub const FIELD_TERMINATOR: u8 = 0x1d;
    pub const LABEL_SEPARATOR: u8 = 0x1e;
    pub const ENVELOPE_SEPARATOR: u8 = 0x1f;
    pub const ENVELOPE_PREFIX: &[u8] = b"OFFER";

    pub const MAX_MESSAGE_LEN: usize = 65_536;
    pub const HARD_MESSAGE_LIMIT: usize = 1_048_576;
    pub const MAX_LABEL_LEN: usize = 256;
    pub const MAX_TEXT_LEN: usize = 256;
    pub const MAX_SAFE_INTEGER: u64 = (1u64 << 53) - 1;

    pub const MASTER_BALL_ITEM_ID: u64 = 4;
    pub const MASTER_BALL_MARKER: u64 = 3;
    pub const MASTER_BALL_STORY_CAP: u64 = 2;
}

pub mod team {
    //! Native team-file version stamps and field kinds.

    pub const CURRENT_VERSION: i32 = 20260802;
    pub const DEFAULT_VERSION: i32 = 20240713;
    pub const LEGACY_OUTFIT_CUTOVER: i32 = 20241217;
    pub const MIN_VERSION: i32 = 20080101;
    pub const MAX_VERSION: i32 = 21001231;
    pub const CUSTOM_OUTFIT_LIMIT: f64 = 210.0;
    pub const INFO_KINDS: [u8; 16] = [0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 2, 2, 0, 0, 0, 0];

    pub const INFO_KIND_NUMERIC: u8 = 0;
    pub const INFO_KIND_TEXT: u8 = 1;
    pub const INFO_KIND_ENCODED: u8 = 2;
}

pub mod world {
    //! Native paths for battle and overworld sprites under the data root.

    pub const BATTLE_SPRITE_DIR: &str = "pkmnbattle";
    pub const BATTLE_SPRITE_EXT: &str = "gmspr";
    pub const OVERWORLD_SPRITE_DIR: &str = "pkmn";
    pub const EGG_SPRITE_DIR: &str = "huevo";
    pub const SHINY_OVERWORLD_SPRITE_DIR: &str = "pkmn_shiny";
    pub const SHINY_EGG_SPRITE_DIR: &str = "huevo_shiny";
    pub const SPRITE_EXT: &str = "gif";

    /// Shiny bands used by the Android party picker. The clean-room world model
    /// covers a wider id space; the picker only ever shows these roster bands.
    pub const SHINY_SPRITE_BANDS: [(i32, i32); 2] = [(1000, 2000), (3000, 4000)];

    pub fn is_shiny_sprite(value: i32) -> bool {
        SHINY_SPRITE_BANDS.iter().any(|&(first, last)| (first..=last).contains(&value))
    }
}

pub mod save {
    //! Native save layout. Slots are adapter policy; the core receives only
    //! success/failure from the save layer.

    pub const ROOT: &str = "data";
    pub const PARTIDAS: &str = "partidas";
    pub const PREONLINE: &str = "preonline";
    pub const PREONLINE_INFIX: &str = "_preonline_";
    pub const SLOT_PREFIX: &str = "reloaded";
    pub const SLOT_MIN: i32 = 1;
    pub const SLOT_MAX: i32 = 99;

    pub const EXT_SAVC: &str = "savc";
    pub const EXT_NFO: &str = "nfo";
    pub const EXT_DATA: &str = "data";
    pub const EXTENSIONS: [&str; 3] = [EXT_SAVC, EXT_NFO, EXT_DATA];

    pub const EXT_SAVB: &str = "savb";
    pub const EXT_SAV: &str = "sav";
    pub const LEGACY_EXTENSIONS: [&str; 2] = [EXT_SAVB, EXT_SAV];

    pub const BACKUP_INFIX: &str = "_bk";
    pub const TEMP_INFIX: &str = "_temp";

    pub const ONLINE_EXT: &str = "online";
    pub const ONLINE_SUFFIX: &str = ".online";

    pub const TEAM_DIR: &str = "equipos";
    pub const TEAM_EXT: &str = "TEAM";

    pub const SAVE_HEADER: [u8; 4] = [0x1d, 0x02, 0x00, 0x00];

    pub const TEMP_SUFFIX: &str = ".tmp";
    pub const WINDOWS_BACKUP_INFIX: &str = ".bak.";

    pub fn slot_stem(slot: i32) -> String {
        format!("{SLOT_PREFIX}{slot}")
    }

    pub fn slot_file_name(slot: i32, extension: &str) -> String {
        format!("{}.{}", slot_stem(slot), extension)
    }

    pub fn backup_file_name(slot: i32, extension: &str) -> String {
        format!("{}{}.{}", slot_stem(slot), BACKUP_INFIX, extension)
    }

    pub fn restore_temp_file_name(slot: i32, extension: &str) -> String {
        format!("{}{}.{}", slot_stem(slot), TEMP_INFIX, extension)
    }

    pub fn is_save_extension(extension: &str) -> bool {
        EXTENSIONS.contains(&extension)
    }

    pub fn is_legacy_save_extension(extension: &str) -> bool {
        LEGACY_EXTENSIONS.contains(&extension)
    }

    pub fn is_save_path(normalized: &str) -> bool {
        EXTENSIONS.iter().any(|extension| normalized.ends_with(&format!(".{extension}")))
    }

    pub fn is_legacy_save_path(normalized: &str) -> bool {
        LEGACY_EXTENSIONS.iter().any(|extension| normalized.ends_with(&format!(".{extension}")))
    }

    pub fn is_online_path(normalized: &str) -> bool {
        normalized.ends_with(ONLINE_SUFFIX)
    }
}

/// Authoritative manifest table. Entries not needed by the current profile are
/// still listed when the adapter can resolve them.
pub const ENTRIES: &[Entry] = &[
    room(room::CENTER, "Centro_Pokemon", "room.center", "pokemon-center"),
    room(room::SELECT, "SelectRoom", "room.select", "character-select"),
    room(room::HOME_1, "Home_1", "room.home.1", "home"),
    room(room::HOME_2, "Home_2", "room.home.2", "home"),
    room(room::EVOLUTION, "PkmnEvolution", "room.evolution", "evolution"),
    room(room::MAP, "MapaMenu", "room.map", "map"),
    room(room::TITLE, "IntroROOM", "room.title", "title"),
    room(room::BOOT, "InicioROOM", "room.boot", "boot"),
    room(room::LOADER, "LoaderROOM", "room.loader", "load"),
    room(room::BATTLE_AQUA, "BattleROOMAqua", "room.battle.water", "battle-water"),
    room(room::BATTLE, "BattleROOM", "room.battle", "battle"),
    room(room::PUZZLE, "PuzleUNW", "room.puzzle", "puzzle"),
    room(room::MULTIPLAYER_MENU, "MultijugadorMenu", "room.multiplayer-menu", "menu"),
    room(room::CREDITS, "CreditosInicioScreen", "room.credits", "menu"),
    room(room::OPTIONS, "OptionsROOM", "room.options", "menu"),
    room(room::LAB_FOLLOWUP, "SecretRoom", "room.cutscene.527", "cutscene"),
    room(room::LAB_INTRO, "CharlaRoom", "room.cutscene.528", "cutscene"),
    room(room::BATTLE_LAVA, "BattleROOMLava", "room.battle.lava", "battle-lava"),
    room(room::SAFARI, "SafariBasic", "room.safari", "safari"),
    object(Some(object::POKEMON_PLAYER), "Pokemon1", "object.pokemon.player", "battle-pokemon-player"),
    object(Some(object::CONTROL), "Control", "object.control", "control"),
    object(Some(object::POKEMON_ENEMY), "Pokemon2", "object.pokemon.enemy", "battle-pokemon-enemy"),
    object(Some(object::TRAINER_PLAYER), "Trainer1", "object.trainer.player", "battle-trainer-player"),
    object(Some(object::TRAINER_ENEMY), "Trainer2", "object.trainer.enemy", "battle-trainer-enemy"),
    object(Some(object::PLAYER), "obj_Prota", "object.player", "player"),
    object(Some(object::VARIABLES), "variables", "object.world-state", "world-state"),
    object(Some(object::EXCLAMATION), "obj_EXC", "object.exclamation", "online-marker"),
    object_aliased(
        Some(object::MULTIPLAYER),
        "MULTIPLAYER",
        "object.multiplayer",
        "multiplayer-controller",
        &["object.player-two"],
    ),
    object(Some(object::INICIO_CONTROL), "InicioControl", "object.inicio-control", "title-controller"),
    script(Some(script::ADJUSTMENTS_ROOM), "AdjustmentsRoom", "script.room-adjustment", "room-adjustment"),
    script(Some(script::CARGANDO_EQUIPO), "CargandoEquipo", "script.team-load", "team-load"),
    script(Some(script::EXECUTE_FILE), "execute_file", "script.execute-file", "file"),
    script(Some(script::FILE_WRITE_DATA), "FileWriteData", "script.file-write-data", "file"),
    script(Some(script::GET_RELOADED_VERSION), "getReloadedVersion", "script.version", "version"),
    script(Some(script::GUARDANDO_EQUIPO), "GuardandoEquipo", "script.team-save", "team-save"),
    script(Some(script::GUARDAR_PARTIDA), "GuardarPartida", "script.save-request", "save-request"),
    script(Some(script::LOAD_CUSTOM_CHARACTERS), "LoadCustomCharacters", "script.custom-characters", "custom-characters"),
    script(Some(script::LOADER_CONTROL_CREATE_SLOTS), "LoaderControlCreateSlots", "script.save-slot-list", "save-slot-list"),
    script(Some(script::LOAD_FILE_INFO), "LoadFileInfo", "script.save-summary-read", "save-summary-read"),
    script(Some(script::LOAD_FILE_SAV), "LoadFileSav", "script.save-read", "save-read"),
    script(Some(script::PRE_SAVE_AC), "PreSaveAc", "script.pre-save-ac", "save"),
    script(Some(script::PRE_SAVE_LINE), "PreSaveLine", "script.pre-save-line", "save"),
    script(Some(script::SAVE_FILE_INFO), "SaveFileInfo", "script.save-summary-write", "save-summary-write"),
    script(Some(script::SAVE_FILE_SAV), "SaveFileSav", "script.save-write", "save-write"),
    script(Some(script::SLOT_BOX_CARGAR), "SlotBoxCargar", "script.slot-load", "slot-load"),
    script(Some(script::SLOT_BOX_DELETE), "SlotBoxDelete", "script.slot-delete", "slot-delete"),
    script(Some(script::SLOT_BOX_RESTORE), "SlotBoxRestore", "script.slot-restore", "slot-restore"),
    script(Some(script::MODAL_DIALOG_PORTRAIT), "Dialogo", "script.modal.dialog-portrait", "modal"),
    script(Some(script::MODAL_DIALOG_MOBILE), "DialogoMovil", "script.modal.dialog-mobile", "modal"),
    script(Some(script::MODAL_DIALOG_PLAIN), "DialogoSimple", "script.modal.dialog-plain", "modal"),
    script(Some(script::MODAL_PREFERENCES), "GetBigSelection", "script.modal.preferences", "modal"),
    script(Some(script::MODAL_TEXT_ENTRY), "GetDialogString", "script.modal.text-entry", "modal"),
    script(Some(script::MODAL_LIST), "GetMenuSelection", "script.modal.list", "modal"),
    script(Some(script::MODAL_DIFFICULTY), "GetSelection", "script.modal.difficulty", "modal"),
    script(Some(script::MODAL_QUESTION), "ShowDialogQuestion", "script.modal.question", "modal"),
    script(Some(script::MODAL_QUESTION_LARGE), "ShowDialogQuestionBig", "script.modal.question-large", "modal"),
    script(None, "DayNightManageCielo", "script.day-night-sky", "visual"),
    script(None, "DrawOutdoorLighting", "script.outdoor-lighting", "visual"),
    script(None, "InitializeCielo", "script.initialize-sky", "visual"),
    script(None, "MultiplayerImportPlayerOne", "script.multiplayer-import-p1", "multiplayer"),
    script(None, "MultiplayerImportPlayerTwo", "script.multiplayer-import-p2", "multiplayer"),
    script(None, "MultiplayerGoBattle", "script.multiplayer-go-battle", "multiplayer"),
    script(None, "ValorTrainerTrucado", "script.trainer-value", "battle"),
    script(None, "SpriteTrainer", "script.trainer-sprite", "battle"),
    script(None, "GetSecurityPK", "script.security-pk.get", "trade"),
    script(None, "GetSecurityPK2", "script.security-pk2.get", "trade"),
    script(None, "SecurityPK", "script.security-pk.apply", "trade"),
    script(None, "SecurityPK2", "script.security-pk2.apply", "trade"),
    script_status(None, "NetTeamSprites", "script.net-team-sprites", "trade", Status::Unsupported),
    global("actual1", "global.actual1", "battle"),
    global("actual2", "global.actual2", "battle"),
    global("apodoName", "global.apodo-name", "player"),
    global("attack1", "global.attack1", "party"),
    global("attack2", "global.attack2", "party"),
    global("battle", "global.battle", "battle-mode"),
    global("battleTurnos", "global.battle-turns", "battle"),
    global("drawModalSystem", "global.modal", "modal"),
    global("ENCAJA", "global.box-open", "storage"),
    global("enemigotrainer", "global.enemy-trainer", "battle"),
    global("enemigotrainer2", "global.enemy-trainer2", "battle"),
    global("enemigotrainer3", "global.enemy-trainer3", "battle"),
    global("ENTR_SPR", "global.entrance-sprite", "battle"),
    global("exp1", "global.exp1", "party"),
    global("exp2", "global.exp2", "party"),
    global("extraInfo", "global.extra-info", "party"),
    global("hierba", "global.grass", "battle-background"),
    global("indexnivel", "global.level-index", "party"),
    global("indexpkmn", "global.species-index", "party"),
    global("info", "global.info", "party"),
    global("info2", "global.info2", "party"),
    global("ISEP2", "global.isep2", "party"),
    global("IV", "global.iv", "party"),
    global("IV2", "global.iv2", "party"),
    global("MAX_PP_1", "global.max-pp1", "party"),
    global("MAX_PS_1", "global.max-hp1", "party"),
    global("MAX_PS_2", "global.max-hp2", "party"),
    global("XTRA_PS_1", "global.extra-hp1", "party"),
    global("atk1", "global.attack-stat1", "party"),
    global("def1", "global.defense-stat1", "party"),
    global("vel1", "global.speed-stat1", "party"),
    global("atk_sp1", "global.special-attack1", "party"),
    global("def_sp1", "global.special-defense1", "party"),
    global("prec1", "global.accuracy1", "party"),
    global("num_attack1", "global.move-count1", "party"),
    global("tipoA1", "global.type-a1", "party"),
    global("tipoB1", "global.type-b1", "party"),
    global("tipoC1", "global.type-c1", "party"),
    global("volador1", "global.flying1", "party"),
    global("fantasma1", "global.ghost1", "party"),
    global("modeBattleBack", "global.battle-back-mode", "battle-background"),
    global("NET_DELAY", "global.net-delay", "online"),
    global("NETW_BROOM", "global.net-battle-room", "online"),
    global("nivel1", "global.level1", "party"),
    global("nivel2", "global.level2", "party"),
    global("numfondobattle", "global.battle-background", "battle-background"),
    global("objpoke1", "global.held-item1", "party"),
    global("objpoke2", "global.held-item2", "party"),
    global("pkmn1", "global.party-species1", "party"),
    global("pkmn2", "global.party-species2", "party"),
    global("pkmnsex1", "global.party-sex1", "party"),
    global("pkmnsex2", "global.party-sex2", "party"),
    global("PLAYER2", "global.player2", "online"),
    global("PP_1", "global.pp1", "party"),
    global("PP_2", "global.pp2", "party"),
    global("prota_ID", "global.player-id", "player"),
    global("prota_index", "global.player-index", "player"),
    global("prota_vestuario", "global.player-outfit", "player"),
    global("protaname", "global.player-name", "player"),
    global("PS_1", "global.hp1", "party"),
    global("PS_2", "global.hp2", "party"),
    global("selue_pk1", "global.selue1", "party"),
    global("selue_pk2", "global.selue2", "party"),
    global("Slot", "global.save-slot", "save"),
    global("sprD", "global.player-sprite", "player"),
    global("STD1", "global.status1", "party"),
    global("STD2", "global.status2", "party"),
    global("TRAINER_NM", "global.trainer-name", "battle"),
    global("TRAINER_SPR", "global.trainer-sprite", "battle"),
    field("BTTBACK_INDEX", "field.battle-back-index", "battle-background"),
    field("char2", "field.character", "player"),
    field("frase2", "field.phrase2", "player-two"),
    field("frasePropia", "field.phrase", "player"),
    field("id2", "field.player-id2", "player-two"),
    field("index2", "field.player-index2", "player-two"),
    field("mask1", "field.selue-mask", "trade"),
    field("NUM_POKEMON1", "field.party-count1", "battle"),
    field("NUM_POKEMON2", "field.party-count2", "battle"),
    field("p2_sprR", "field.player-two-sprite", "battle"),
    field("PKMN1", "field.party-species1", "battle"),
    field("PKMN2", "field.party-species2", "battle"),
    field("PLAYER1_INI", "field.player-one-ready", "battle"),
    field("PLAYER2_INI", "field.player-two-ready", "battle"),
    field("protaname2", "field.player-name2", "player-two"),
    field("SEL", "field.menu-selection", "title"),
    field("sp_char2", "field.character2", "player-two"),
    field("sprINDEX", "field.sprite-index", "battle"),
    field("sprINDEX2", "field.sprite-index2", "battle"),
    field("sprR", "field.sprite-result", "battle"),
    sprite(sprite::BALL, "spr_ball", "sprite.ball", "online-battle-marker"),
    sprite(sprite::PLAYER_DEFAULT, "Mark2_D", "sprite.player-default", "player"),
    save_path("data/partidas", "save.root", "save-root"),
    save_path("data/partidas/preonline", "save.preonline", "save-backup"),
    save_path("data/partidas/reloaded<N>.savc", "save.slot.savc", "save-slot"),
    save_path("data/partidas/reloaded<N>.nfo", "save.slot.nfo", "save-summary"),
    save_path("data/partidas/reloaded<N>.data", "save.slot.data", "save-sidecar"),
    save_path("data/partidas/reloaded<N>.savb", "save.slot.savb", "legacy-save"),
    save_path("data/partidas/reloaded<N>.sav", "save.slot.sav", "legacy-save"),
    save_path("data/partidas/reloaded<N>_bk.<ext>", "save.slot.backup", "save-backup"),
    save_path("data/partidas/reloaded<N>_temp.<ext>", "save.slot.restore-temp", "save-restore-temp"),
    save_path("data/partidas/reloaded<N>.savc.online", "save.online-overlay", "save-overlay"),
    save_path("data/equipos/<name>.TEAM", "save.team-file", "team"),
    save_path("data/char_custom/<character>.bmp", "world.custom-character", "custom-character"),
    save_path("LogBattle.txt", "save.log-battle", "battle-log"),
    save_path("data/pkmnbattle/P<id>_<slot>.gmspr", "world.battle-sprite", "battle-sprite"),
    save_path("data/pkmnbattle/P<id>_<slot>_ALT.gmspr", "world.battle-sprite-alt", "battle-sprite"),
    save_path("data/pkmn/<id>.gif", "world.overworld-sprite", "overworld-sprite"),
    save_path("data/pkmn_shiny/<id>.gif", "world.shiny-overworld-sprite", "overworld-sprite"),
    save_path("data/huevo/<id>.gif", "world.egg-sprite", "egg-sprite"),
    save_path("data/huevo_shiny/<id>.gif", "world.shiny-egg-sprite", "egg-sprite"),

];

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

const fn digest_u32(mut hash: u64, value: u32) -> u64 {
    let bytes = value.to_le_bytes();
    let mut index = 0;
    while index < bytes.len() {
        hash = (hash ^ bytes[index] as u64).wrapping_mul(FNV_PRIME);
        index += 1;
    }
    hash
}

const fn digest_str(mut hash: u64, text: &str) -> u64 {
    let bytes = text.as_bytes();
    hash = digest_u32(hash, bytes.len() as u32);
    let mut index = 0;
    while index < bytes.len() {
        hash = (hash ^ bytes[index] as u64).wrapping_mul(FNV_PRIME);
        index += 1;
    }
    hash
}

const fn kind_tag(kind: Kind) -> u32 {
    match kind {
        Kind::Room => 1,
        Kind::Object => 2,
        Kind::Script => 3,
        Kind::Global => 4,
        Kind::Field => 5,
        Kind::Sprite => 6,
        Kind::SavePath => 7,
    }
}

const fn status_tag(status: Status) -> u32 {
    match status {
        Status::Supported => 1,
        Status::Unsupported => 2,
        Status::DisplayOnly => 3,
    }
}

const fn digest_entry(mut hash: u64, entry: &Entry) -> u64 {
    hash = digest_u32(hash, kind_tag(entry.kind));
    hash = digest_str(hash, entry.native_name);
    hash = digest_str(hash, entry.content_key);
    match entry.native_id {
        Some(id) => {
            hash = digest_u32(hash, 1);
            hash = digest_u32(hash, id as u32);
        },
        None => hash = digest_u32(hash, 0),
    }
    hash = digest_u32(hash, entry.aliases.len() as u32);
    let mut index = 0;
    while index < entry.aliases.len() {
        hash = digest_str(hash, entry.aliases[index]);
        index += 1;
    }
    match entry.role {
        Some(role) => {
            hash = digest_u32(hash, 1);
            hash = digest_str(hash, role);
        },
        None => hash = digest_u32(hash, 0),
    }
    digest_u32(hash, status_tag(entry.status))
}

const fn manifest_digest() -> u64 {
    let mut hash = FNV_OFFSET;
    hash = digest_u32(hash, BUILD.manifest);
    hash = digest_str(hash, BUILD.family);
    hash = digest_str(hash, BUILD.stamp);
    hash = digest_u32(hash, BUILD.rooms);
    hash = digest_u32(hash, BUILD.objects);
    hash = digest_u32(hash, BUILD.scripts);
    hash = digest_u32(hash, BUILD.globals);
    hash = digest_u32(hash, BUILD.save_slots);
    let mut index = 0;
    while index < ENTRIES.len() {
        hash = digest_entry(hash, &ENTRIES[index]);
        index += 1;
    }
    hash
}

/// Stable FNV-1a digest of the build info and every manifest entry. It changes
/// only when a native id, name, key, alias, role or status changes.
pub const MANIFEST_DIGEST: u64 = manifest_digest();

/// Returns the stable manifest digest.
pub fn stable_digest() -> u64 {
    MANIFEST_DIGEST
}

/// Alias for [`stable_digest`].
pub fn digest() -> u64 {
    MANIFEST_DIGEST
}

/// Finds a manifest entry by its stable content key or an alias.
pub fn entry(content_key: &str) -> Option<&'static Entry> {
    ENTRIES
        .iter()
        .find(|entry| entry.content_key == content_key || entry.aliases.contains(&content_key))
}

/// Finds a manifest entry by native namespace, id, and name. When an id is
/// supplied the native name must match too, so a stale id in core code cannot
/// silently resolve to a different asset.
pub fn native(kind: Kind, id: Option<i32>, name: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| {
        entry.kind == kind
            && entry.native_name == name
            && match id {
                Some(id) => entry.native_id == Some(id),
                None => true,
            }
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManifestCounts {
    pub rooms: usize,
    pub objects: usize,
    pub scripts: usize,
    pub globals: usize,
    pub fields: usize,
    pub sprites: usize,
    pub save_paths: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestError {
    BuildMismatch,
    DeclaredCount,
    EmptyField,
    PlaceholderName,
    MissingRole,
    DuplicateKey,
    DuplicateAlias,
    AliasCollision,
    DuplicateNativeId,
    DuplicateNativeName,
}

/// Counts the manifest entries of each namespace.
pub fn entry_counts() -> ManifestCounts {
    let mut counts =
        ManifestCounts { rooms: 0, objects: 0, scripts: 0, globals: 0, fields: 0, sprites: 0, save_paths: 0 };
    for entry in ENTRIES {
        match entry.kind {
            Kind::Room => counts.rooms += 1,
            Kind::Object => counts.objects += 1,
            Kind::Script => counts.scripts += 1,
            Kind::Global => counts.globals += 1,
            Kind::Field => counts.fields += 1,
            Kind::Sprite => counts.sprites += 1,
            Kind::SavePath => counts.save_paths += 1,
        }
    }
    counts
}

/// Checks the manifest against the declared build family and its own integrity:
/// no namespace exceeds the declared native build, every namespace is
/// represented, keys/aliases/native identifiers are unique, and every entry has
/// an exact name, key and role.
pub fn validate() -> Result<ManifestCounts, ManifestError> {
    use std::collections::HashSet;

    if BUILD.manifest != MANIFEST_VERSION || BUILD.family.is_empty() || BUILD.stamp.is_empty() {
        return Err(ManifestError::BuildMismatch)
    }

    let counts = entry_counts();
    if counts.rooms == 0
        || counts.objects == 0
        || counts.scripts == 0
        || counts.globals == 0
        || counts.fields == 0
        || counts.sprites == 0
        || counts.save_paths == 0
        || counts.rooms as u32 > BUILD.rooms
        || counts.objects as u32 > BUILD.objects
        || counts.scripts as u32 > BUILD.scripts
        || counts.globals as u32 > BUILD.globals
        || counts.save_paths as u32 > BUILD.save_slots
    {
        return Err(ManifestError::DeclaredCount)
    }

    let mut keys = HashSet::new();
    let mut aliases = HashSet::new();
    let mut native_ids = HashSet::new();
    let mut native_names = HashSet::new();
    for entry in ENTRIES {
        if entry.native_name.is_empty() || entry.content_key.is_empty() || entry.role.is_none() {
            return Err(ManifestError::EmptyField)
        }
        if entry.native_name.starts_with("unknown#") {
            return Err(ManifestError::PlaceholderName)
        }
        if !keys.insert(entry.content_key) {
            return Err(ManifestError::DuplicateKey)
        }
        for alias in entry.aliases {
            if alias.is_empty() {
                return Err(ManifestError::EmptyField)
            }
            if !aliases.insert(*alias) {
                return Err(ManifestError::DuplicateAlias)
            }
        }
        if let Some(id) = entry.native_id {
            if !native_ids.insert((entry.kind, id)) {
                return Err(ManifestError::DuplicateNativeId)
            }
        }
        if !native_names.insert((entry.kind, entry.native_name)) {
            return Err(ManifestError::DuplicateNativeName)
        }
    }
    for entry in ENTRIES {
        for alias in entry.aliases {
            if keys.contains(alias) {
                return Err(ManifestError::AliasCollision)
            }
        }
    }

    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn manifest_digest_is_stable() {
        assert_eq!(stable_digest(), MANIFEST_DIGEST);
        assert_eq!(digest(), MANIFEST_DIGEST);
        assert_eq!(MANIFEST_DIGEST, 0x9791_9649_3608_a887);
    }

    #[test]
    fn manifest_validates_against_declared_build() {
        let counts = validate().expect("manifest must validate");
        assert_eq!(
            counts,
            ManifestCounts {
                rooms: 19,
                objects: 10,
                scripts: 40,
                globals: 69,
                fields: 20,
                sprites: 2,
                save_paths: 19,
            }
        );
        assert_eq!(counts.rooms as u32, 19);
        assert!(counts.rooms as u32 <= BUILD.rooms);
        assert!(counts.objects as u32 <= BUILD.objects);
        assert!(counts.scripts as u32 <= BUILD.scripts);
        assert!(counts.globals as u32 <= BUILD.globals);
        assert!(counts.save_paths as u32 <= BUILD.save_slots);
    }

    #[test]
    fn manifest_declares_corrected_title_and_menu_ids() {
        assert_eq!(
            native(Kind::Object, Some(98), "InicioControl").map(|entry| entry.content_key),
            Some("object.inicio-control")
        );
        assert_eq!(
            native(Kind::Room, Some(465), "OptionsROOM").map(|entry| entry.content_key),
            Some("room.options")
        );
        assert_eq!(
            native(Kind::Room, Some(466), "CreditosInicioScreen").map(|entry| entry.content_key),
            Some("room.credits")
        );
        assert!(native(Kind::Room, Some(465), "CreditosInicioScreen").is_none());
        assert!(native(Kind::Room, Some(466), "OptionsROOM").is_none());
    }

    #[test]
    fn manifest_marks_net_team_sprites_unsupported() {
        let entry = native(Kind::Script, None, "NetTeamSprites").expect("NetTeamSprites entry");
        assert_eq!(entry.status, Status::Unsupported);
        assert_eq!(entry.content_key, "script.net-team-sprites");
    }

    #[test]
    fn manifest_declares_legacy_save_paths() {
        for key in [
            "save.slot.savb",
            "save.slot.sav",
            "save.slot.backup",
            "save.slot.restore-temp",
            "world.custom-character",
            "save.log-battle",
        ] {
            assert!(entry(key).is_some(), "missing legacy path {key}");
        }
        assert_eq!(save::backup_file_name(7, save::EXT_SAVC), "reloaded7_bk.savc");
        assert_eq!(save::restore_temp_file_name(7, save::EXT_NFO), "reloaded7_temp.nfo");
        assert!(save::is_legacy_save_extension(save::EXT_SAVB));
        assert!(save::is_legacy_save_extension(save::EXT_SAV));
        assert!(!save::is_legacy_save_extension(save::EXT_SAVC));
        assert!(save::is_legacy_save_path("data/partidas/reloaded1.sav"));
        assert!(!save::is_legacy_save_path("data/partidas/reloaded1.savc"));
    }

    #[test]
    fn native_requires_matching_id_and_name() {
        assert!(native(Kind::Object, Some(1172), "MULTIPLAYER").is_some());
        assert!(native(Kind::Object, Some(1172), "obj_Prota").is_none());
        assert!(native(Kind::Object, Some(14), "obj_Prota").is_some());
        assert!(native(Kind::Object, Some(99), "obj_Prota").is_none());
        assert!(native(Kind::Object, None, "obj_Prota").is_some());
        assert!(native(Kind::Object, None, "missing").is_none());
    }

    #[test]
    fn manifest_alias_resolution_is_unambiguous() {
        assert_eq!(
            entry("object.player-two").map(|entry| entry.content_key),
            Some("object.multiplayer")
        );
        assert_eq!(entry("object.multiplayer").map(|entry| entry.content_key), Some("object.multiplayer"));
        assert!(validate().is_ok());
    }

    #[test]
    fn manifest_has_no_placeholder_names() {
        for entry in ENTRIES {
            assert!(!entry.native_name.starts_with("unknown#"), "placeholder name {}", entry.native_name);
        }
    }

    #[test]
    fn manifest_keys_are_unique() {
        let mut keys = HashSet::new();
        for entry in ENTRIES {
            assert!(keys.insert(entry.content_key), "duplicate content key {}", entry.content_key);
        }
    }

    #[test]
    fn manifest_native_identifiers_are_unique_per_kind() {
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for entry in ENTRIES {
            if let Some(id) = entry.native_id {
                assert!(ids.insert((entry.kind, id)), "duplicate {:?} id {id}", entry.kind);
            }
            assert!(
                names.insert((entry.kind, entry.native_name)),
                "duplicate {:?} name {}",
                entry.kind,
                entry.native_name
            );
        }
    }

    #[test]
    fn save_slot_layout_is_manifest_owned() {
        assert_eq!(save::slot_stem(9), format!("{}{}", save::SLOT_PREFIX, 9));
        assert_eq!(save::slot_file_name(9, save::EXT_SAVC), "reloaded9.savc");
        assert_eq!(save::backup_file_name(9, save::EXT_SAVC), "reloaded9_bk.savc");
        assert_eq!(save::restore_temp_file_name(9, save::EXT_SAVC), "reloaded9_temp.savc");
        assert!(save::is_save_path("data/partidas/reloaded9.savc"));
        assert!(!save::is_save_path("data/partidas/reloaded9.txt"));
    }

    #[test]
    fn manifest_entries_cover_instance_fields() {
        let fields = [
            field::BTTBACK_INDEX,
            field::CHAR2,
            field::FRASE2,
            field::FRASE_PROPIA,
            field::ID2,
            field::INDEX2,
            field::MASK1,
            field::NUM_POKEMON1,
            field::NUM_POKEMON2,
            field::P2_SPRR,
            field::PKMN1,
            field::PKMN2,
            field::PLAYER1_INI,
            field::PLAYER2_INI,
            field::PROTANAME2,
            field::SEL,
            field::SP_CHAR2,
            field::SPRINDEX,
            field::SPRINDEX2,
            field::SPR_R,
        ];
        for name in fields {
            let name = std::str::from_utf8(name).unwrap();
            assert!(native(Kind::Field, None, name).is_some(), "missing field entry {name}");
        }
    }

    #[test]
    fn manifest_entries_cover_world_paths_and_default_sprite() {
        for key in [
            "save.team-file",
            "world.battle-sprite",
            "world.battle-sprite-alt",
            "world.overworld-sprite",
            "world.shiny-overworld-sprite",
            "world.egg-sprite",
            "world.shiny-egg-sprite",
        ] {
            assert!(entry(key).is_some(), "missing path entry {key}");
        }
        assert_eq!(
            native(Kind::Sprite, Some(player::DEFAULT_SPRITE), "Mark2_D").map(|entry| entry.content_key),
            Some("sprite.player-default")
        );
    }

    #[test]
    fn shiny_picker_bands_match_native_behavior() {
        assert!(!world::is_shiny_sprite(999));
        assert!(world::is_shiny_sprite(1000));
        assert!(world::is_shiny_sprite(2000));
        assert!(!world::is_shiny_sprite(2001));
        assert!(!world::is_shiny_sprite(2999));
        assert!(world::is_shiny_sprite(3000));
        assert!(world::is_shiny_sprite(4000));
        assert!(!world::is_shiny_sprite(4001));
    }

    #[test]
    fn player_sprite_index_matches_native_convention() {
        assert!(player::is_custom(player::CUSTOM_INDEX));
        assert!(!player::is_custom(player::CUSTOM_INDEX + 1));
        assert_eq!(player::sprite_index(25, -1), 25 + player::SPRITE_OFFSET);
        assert_eq!(player::sprite_index(player::CUSTOM_INDEX, 77), 77);
    }

    #[test]
    fn room_helpers_match_the_native_names() {
        assert!(room::is_no_player_name("iniciowhatever"));
        assert!(room::is_no_player_name("selectroom"));
        assert!(room::is_no_player_name("intro"));
        assert!(room::is_charla_name("charla"));
        assert!(room::is_menu_name("optionsroom"));
        assert!(room::is_bag_name("mochila"));
        assert!(room::is_options_name("opcion"));
    }
}
