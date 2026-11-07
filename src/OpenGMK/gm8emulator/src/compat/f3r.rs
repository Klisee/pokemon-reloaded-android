//! F3R (Pokemon Reloaded) compatibility manifest.
//!
//! Every native room, object, script, global and save-path identifier used by the
//! emulator is declared here. Core code addresses content through these content
//! keys and constants; it must not carry native names or numeric ids of its own.
//!
//! Numeric ids are build-scoped. A different F3R build needs a new manifest
//! revision rather than core changes.

/// Manifest revision. Increment when native ids, names, or the supported build
/// family change.
pub const MANIFEST_VERSION: u32 = 1;

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
    Entry {
        kind: Kind::Object,
        native_id: id,
        native_name: name,
        content_key: key,
        aliases: &[],
        role: Some(role),
        status: Status::Supported,
    }
}

const fn script(id: Option<i32>, name: &'static str, key: &'static str, role: &'static str) -> Entry {
    Entry {
        kind: Kind::Script,
        native_id: id,
        native_name: name,
        content_key: key,
        aliases: &[],
        role: Some(role),
        status: Status::Supported,
    }
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
    pub const CREDITS: i32 = 465;
    pub const OPTIONS: i32 = 466;
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
    pub const PLAYER_TWO: i32 = 1172;

    pub const PLAYER_NAME: &[u8] = b"obj_Prota";
    pub const INICIO_CONTROL_NAME: &[u8] = b"InicioControl";
    pub const MULTIPLAYER_NAME: &[u8] = b"MULTIPLAYER";

    pub fn is_player_name(name: &[u8]) -> bool {
        name == PLAYER_NAME
    }

    pub fn is_inicio_control_name(name: &[u8]) -> bool {
        name == INICIO_CONTROL_NAME
    }

    pub fn is_multiplayer_name(name: &[u8]) -> bool {
        name == MULTIPLAYER_NAME
    }
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
    pub const HIERBA: &[u8] = b"hierba";
    pub const INDEXNIVEL: &[u8] = b"indexnivel";
    pub const INDEXPKMN: &[u8] = b"indexpkmn";
    pub const INFO: &[u8] = b"info";
    pub const INFO2: &[u8] = b"info2";
    pub const ISEP2: &[u8] = b"ISEP2";
    pub const IV: &[u8] = b"IV";
    pub const IV2: &[u8] = b"IV2";
    pub const MAX_PS_1: &[u8] = b"MAX_PS_1";
    pub const MAX_PS_2: &[u8] = b"MAX_PS_2";
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

    pub const ONLINE_EXT: &str = "online";
    pub const ONLINE_SUFFIX: &str = ".online";

    pub fn slot_stem(slot: i32) -> String {
        format!("{SLOT_PREFIX}{slot}")
    }

    pub fn slot_file_name(slot: i32, extension: &str) -> String {
        format!("{}.{}", slot_stem(slot), extension)
    }

    pub fn is_save_extension(extension: &str) -> bool {
        EXTENSIONS.contains(&extension)
    }

    pub fn is_save_path(normalized: &str) -> bool {
        EXTENSIONS.iter().any(|extension| normalized.ends_with(&format!(".{extension}")))
    }

    pub fn is_online_path(normalized: &str) -> bool {
        normalized.ends_with(ONLINE_SUFFIX)
    }
}

/// Authoritative manifest table. Entries not needed by the current profile are
/// still listed when the adapter can resolve them.
pub static ENTRIES: &[Entry] = &[
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
    room(room::LAB_FOLLOWUP, "unknown#527", "room.cutscene.527", "cutscene"),
    room(room::LAB_INTRO, "unknown#528", "room.cutscene.528", "cutscene"),
    room(room::BATTLE_LAVA, "BattleROOMLava", "room.battle.lava", "battle-lava"),
    room(room::SAFARI, "unknown#715", "room.safari", "safari"),
    object(Some(object::POKEMON_PLAYER), "Pokemon1", "object.pokemon.player", "battle-pokemon-player"),
    object(Some(object::CONTROL), "Control", "object.control", "control"),
    object(Some(object::POKEMON_ENEMY), "Pokemon2", "object.pokemon.enemy", "battle-pokemon-enemy"),
    object(Some(object::TRAINER_PLAYER), "Trainer1", "object.trainer.player", "battle-trainer-player"),
    object(Some(object::TRAINER_ENEMY), "Trainer2", "object.trainer.enemy", "battle-trainer-enemy"),
    object(Some(object::PLAYER), "obj_Prota", "object.player", "player"),
    object(Some(object::VARIABLES), "variables", "object.world-state", "world-state"),
    object(Some(object::PLAYER_TWO), "unknown#1172", "object.player-two", "player-two"),
    object(None, "InicioControl", "object.inicio-control", "title-controller"),
    object(None, "MULTIPLAYER", "object.multiplayer", "multiplayer-controller"),
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
    script(Some(script::MODAL_DIALOG_PORTRAIT), "unknown#6937", "script.modal.dialog-portrait", "modal"),
    script(Some(script::MODAL_DIALOG_MOBILE), "unknown#6941", "script.modal.dialog-mobile", "modal"),
    script(Some(script::MODAL_DIALOG_PLAIN), "unknown#6949", "script.modal.dialog-plain", "modal"),
    script(Some(script::MODAL_PREFERENCES), "unknown#7744", "script.modal.preferences", "modal"),
    script(Some(script::MODAL_TEXT_ENTRY), "unknown#7754", "script.modal.text-entry", "modal"),
    script(Some(script::MODAL_LIST), "unknown#7778", "script.modal.list", "modal"),
    script(Some(script::MODAL_DIFFICULTY), "unknown#7836", "script.modal.difficulty", "modal"),
    script(Some(script::MODAL_QUESTION), "unknown#9673", "script.modal.question", "modal"),
    script(Some(script::MODAL_QUESTION_LARGE), "unknown#9674", "script.modal.question-large", "modal"),
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
    script(None, "NetTeamSprites", "script.net-team-sprites", "trade"),
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
    global("hierba", "global.grass", "battle-background"),
    global("indexnivel", "global.level-index", "party"),
    global("indexpkmn", "global.species-index", "party"),
    global("info", "global.info", "party"),
    global("info2", "global.info2", "party"),
    global("ISEP2", "global.isep2", "party"),
    global("IV", "global.iv", "party"),
    global("IV2", "global.iv2", "party"),
    global("MAX_PS_1", "global.max-hp1", "party"),
    global("MAX_PS_2", "global.max-hp2", "party"),
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
    save_path("data/partidas", "save.root", "save-root"),
    save_path("data/partidas/preonline", "save.preonline", "save-backup"),
    save_path("data/partidas/reloaded<N>.savc", "save.slot.savc", "save-slot"),
    save_path("data/partidas/reloaded<N>.nfo", "save.slot.nfo", "save-summary"),
    save_path("data/partidas/reloaded<N>.data", "save.slot.data", "save-sidecar"),
    save_path("data/partidas/reloaded<N>.savc.online", "save.online-overlay", "save-overlay"),
];

/// Finds a manifest entry by its stable content key.
pub fn entry(content_key: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| entry.content_key == content_key)
}

/// Finds a manifest entry by native namespace, id, or name.
pub fn native(kind: Kind, id: Option<i32>, name: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| {
        entry.kind == kind
            && match id {
                Some(id) => entry.native_id == Some(id),
                None => entry.native_name == name,
            }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
        assert!(save::is_save_path("data/partidas/reloaded9.savc"));
        assert!(!save::is_save_path("data/partidas/reloaded9.txt"));
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
