use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

pub type Revision = u64;
pub type Tick = u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Timestamp(pub i64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Digest(pub [u8; 32]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PeerId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SessionId(pub [u8; 16]);

#[derive(Clone, Copy, Debug)]
pub enum Wait {
    Now,
    For(Duration),
    Forever,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    Unavailable,
    Disconnected,
    Timeout,
    Cancelled,
    Corrupt,
    UnsupportedVersion,
    Conflict,
    Rejected,
    Protocol,
    Overloaded,
    Io,
    Internal,
}

pub trait PortError: std::error::Error + Send + Sync + 'static {
    fn kind(&self) -> ErrorKind;
    fn retryable(&self) -> bool;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortInfo {
    pub protocol: Version,
    pub schema: Version,
    pub read_only: bool,
    pub features: Vec<String>,
}

pub trait Port {
    fn info(&self) -> PortInfo;
}

pub trait Stream<T>: Send {
    type Error: PortError;

    fn next(&mut self, wait: Wait) -> Result<Option<T>, Self::Error>;
    fn close(&mut self) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PartySlot(pub u8);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PartyChange {
    pub revision: Revision,
    pub kind: PartyChangeKind,
    pub slots: Vec<PartySlot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PartyChangeKind {
    Added,
    Removed,
    Replaced,
    Reordered,
    ActiveChanged,
    Reset,
}

pub trait PartyMember: Clone + Send + Sync + 'static {
    type Id: Clone + Eq + std::hash::Hash + Send + Sync + 'static;

    fn id(&self) -> Self::Id;
    fn level(&self) -> u16;
    fn is_egg(&self) -> bool;
    fn is_fainted(&self) -> bool;
}

#[derive(Clone, Debug)]
pub struct PartyView<M> {
    pub revision: Revision,
    pub capacity: u8,
    pub active: Option<PartySlot>,
    pub slots: Vec<Option<M>>,
}

#[derive(Clone, Debug)]
pub enum PartyCommand<M> {
    SetActive { slot: PartySlot },
    Swap { a: PartySlot, b: PartySlot },
    Add { member: M, slot: Option<PartySlot> },
    Remove { slot: PartySlot },
    Replace { slot: PartySlot, member: M },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartyReceipt {
    pub previous: Revision,
    pub current: Revision,
    pub changed: Vec<PartySlot>,
}

pub trait PartyAccess: Port + Send + Sync {
    type Member: PartyMember;
    type Error: PortError;

    fn view(&self) -> Result<PartyView<Self::Member>, Self::Error>;
    fn member(&self, slot: PartySlot) -> Result<Option<Self::Member>, Self::Error>;
    fn subscribe(&self) -> Result<Box<dyn Stream<PartyChange, Error = Self::Error>>, Self::Error>;
    fn apply(
        &self,
        command: PartyCommand<Self::Member>,
        expected: Revision,
    ) -> Result<PartyReceipt, Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct WorldId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SaveSlot(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SnapshotId(pub [u8; 16]);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FormatId(pub [u8; 8]);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveSnapshot {
    pub id: SnapshotId,
    pub world: WorldId,
    pub slot: SaveSlot,
    pub format: FormatId,
    pub schema: Version,
    pub revision: Revision,
    pub captured_at: Timestamp,
    pub digest: Digest,
    pub body: Arc<[u8]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotRequest {
    Current,
    Slot(SaveSlot),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SaveTarget {
    pub slot: SaveSlot,
    pub mode: SaveMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveMode {
    Replace,
    FailIfExists,
    VerifyOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compatibility {
    Writable,
    ReadOnly,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveSlotInfo {
    pub slot: SaveSlot,
    pub world: WorldId,
    pub format: FormatId,
    pub schema: Version,
    pub revision: Revision,
    pub modified_at: Timestamp,
    pub size: u64,
    pub digest: Digest,
    pub compatibility: Compatibility,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotVerification {
    pub valid: bool,
    pub compatibility: Compatibility,
    pub digest_matches: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveCommit {
    pub slot: SaveSlot,
    pub previous: Revision,
    pub current: Revision,
    pub digest: Digest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoreReceipt {
    pub previous: Revision,
    pub current: Revision,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SaveEvent {
    Captured { id: SnapshotId, revision: Revision },
    Verified { id: SnapshotId, valid: bool },
    Committed { slot: SaveSlot, revision: Revision },
    Restored { slot: SaveSlot, revision: Revision },
    Invalidated { slot: SaveSlot, reason: String },
}

pub trait SaveSnapshots: Port + Send + Sync {
    type Error: PortError;

    fn slots(&self) -> Result<Vec<SaveSlotInfo>, Self::Error>;
    fn capture(&self, request: SnapshotRequest) -> Result<SaveSnapshot, Self::Error>;
    fn verify(&self, snapshot: &SaveSnapshot) -> Result<SnapshotVerification, Self::Error>;
    fn commit(&self, snapshot: &SaveSnapshot, target: SaveTarget) -> Result<SaveCommit, Self::Error>;
    fn restore(&self, snapshot: &SaveSnapshot, expected: Revision) -> Result<RestoreReceipt, Self::Error>;
    fn subscribe(&self) -> Result<Box<dyn Stream<SaveEvent, Error = Self::Error>>, Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TradeSessionId(pub [u8; 16]);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TradeFrame {
    pub session: TradeSessionId,
    pub sequence: u64,
    pub payload: Arc<[u8]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendOutcome {
    Sent,
    Backpressured,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseReason {
    Complete,
    Cancelled,
    Timeout,
    Protocol,
    Transport,
}

pub const MAX_TRADE_FRAME: usize = 64 * 1024;

pub trait TradeTransport: Port + Send + Sync {
    type Error: PortError;
    type Link: TradeLink<Error = Self::Error>;

    fn open(&self, peer: PeerId, wait: Wait) -> Result<Self::Link, Self::Error>;
    fn accept(&self, wait: Wait) -> Result<Option<Self::Link>, Self::Error>;
}

pub trait TradeLink: Send {
    type Error: PortError;

    fn session(&self) -> TradeSessionId;
    fn peer(&self) -> PeerId;
    fn send(&mut self, frame: TradeFrame, wait: Wait) -> Result<SendOutcome, Self::Error>;
    fn receive(&mut self, wait: Wait) -> Result<Option<TradeFrame>, Self::Error>;
    fn close(&mut self, reason: CloseReason) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct MessageId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct MessageKind(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CorrelationId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MessageSource {
    Core,
    Adapter,
    Script,
    Peer(PeerId),
}

pub trait WorldMessage: Clone + Send + Sync + 'static {
    fn kind(&self) -> MessageKind;
    fn schema(&self) -> Version;
}

#[derive(Clone, Debug)]
pub struct WorldEnvelope<M> {
    pub id: MessageId,
    pub tick: Tick,
    pub source: MessageSource,
    pub correlation: Option<CorrelationId>,
    pub message: M,
}

#[derive(Clone, Debug)]
pub enum WorldEvent<M> {
    Message(WorldEnvelope<M>),
    Gap { after: MessageId, before: MessageId },
    Overflow { dropped: u64 },
}

#[derive(Clone, Debug)]
pub struct MessageFilter {
    pub kinds: Option<Vec<MessageKind>>,
    pub sources: Option<Vec<MessageSource>>,
    pub correlation: Option<CorrelationId>,
}

pub trait WorldMessages: Port + Send + Sync {
    type Message: WorldMessage;
    type Error: PortError;

    fn publish(&self, message: Self::Message) -> Result<MessageId, Self::Error>;
    fn subscribe(
        &self,
        filter: MessageFilter,
    ) -> Result<Box<dyn Stream<WorldEvent<Self::Message>, Error = Self::Error>>, Self::Error>;
}

pub type StoryKey = u32;
pub type StoryValue = i64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoryState {
    pub revision: Revision,
    pub phase: u32,
    pub flags: BTreeMap<StoryKey, StoryValue>,
    pub counters: BTreeMap<StoryKey, i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoryRequest {
    pub action: StoryAction,
    pub context: StoryContext,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoryAction {
    Check(StoryKey),
    Enter(StoryKey),
    Interact(StoryKey),
    Obtain { key: StoryKey, count: u32 },
    Defeat(StoryKey),
    Custom { kind: u32, payload: Vec<u8> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoryContext {
    pub area: Option<u32>,
    pub actor: Option<u32>,
    pub target: Option<u32>,
    pub tags: BTreeMap<StoryKey, StoryValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoryDecision {
    Allow,
    AllowWith(Vec<StoryEffect>),
    Deny(StoryDenial),
    Defer(StoryCondition),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoryEffect {
    pub key: StoryKey,
    pub op: StoryOp,
    pub value: StoryValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoryOp {
    Set,
    Add,
    Clear,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoryDenial {
    Locked { key: StoryKey },
    Missing { key: StoryKey, needed: StoryValue, actual: StoryValue },
    OutOfOrder { expected_phase: u32, actual_phase: u32 },
    Custom { kind: u32, payload: Vec<u8> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoryCondition {
    Flag { key: StoryKey, equals: StoryValue },
    CounterAtLeast { key: StoryKey, value: i64 },
    Phase(u32),
    Custom { kind: u32, payload: Vec<u8> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoryEvent {
    AreaEntered(u32),
    Interacted(u32),
    ItemObtained { key: StoryKey, count: u32 },
    TrainerDefeated(u32),
    FlagChanged { key: StoryKey, value: StoryValue },
    Custom { kind: u32, payload: Vec<u8> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoryTransition {
    pub state: StoryState,
    pub effects: Vec<StoryEffect>,
    pub changed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoryValidation {
    pub valid: bool,
    pub warnings: Vec<String>,
}

pub trait StoryPolicy: Port + Send + Sync {
    type Error: PortError;

    fn decide(
        &self,
        state: &StoryState,
        request: &StoryRequest,
    ) -> Result<StoryDecision, Self::Error>;

    fn advance(
        &self,
        state: &StoryState,
        event: &StoryEvent,
    ) -> Result<StoryTransition, Self::Error>;

    fn validate(&self, state: &StoryState) -> Result<StoryValidation, Self::Error>;
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ContentKey(pub Arc<str>);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ContentKind {
    Room,
    Object,
    Script,
    Global,
    SavePath,
    ContentString,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ContentStatus {
    Supported,
    Unsupported,
    DisplayOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SemanticRole {
    Boot,
    Title,
    Load,
    CharacterSelect,
    Map,
    Battle,
    Player,
    WorldState,
    Overlay,
    Interaction,
    Collision,
    SaveRequest,
    SaveWrite,
    SaveRead,
    TeamLoad,
    TeamSave,
    SlotList,
    SlotLoad,
    SlotDelete,
    SlotRestore,
    ContentLookup,
    Transition,
}

pub trait OpaqueHandle: Clone + std::fmt::Debug + Eq + std::hash::Hash + Send + Sync + 'static {}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ContentRecord<H: OpaqueHandle> {
    pub kind: ContentKind,
    pub handle: H,
    pub key: ContentKey,
    pub aliases: Vec<ContentKey>,
    pub role: Option<SemanticRole>,
    pub status: ContentStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoomDescriptor {
    pub width: u32,
    pub height: u32,
    pub display_name: ContentKey,
    pub role: Option<SemanticRole>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NormalizedValue {
    Null,
    Bool(bool),
    Integer(i64),
    Fixed { scaled: i64, scale: u32 },
    Text(Arc<str>),
    Bytes(Arc<[u8]>),
    List(Vec<NormalizedValue>),
    Map(BTreeMap<ContentKey, NormalizedValue>),
}

pub type ScriptArgs = Vec<NormalizedValue>;

#[derive(Clone, Debug)]
pub enum ScriptResult {
    None,
    Value(NormalizedValue),
    Deferred(Wait),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentText {
    pub text: Arc<str>,
    pub locale: Arc<str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct AssetHandle(pub [u8; 16]);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationReport {
    pub valid: bool,
    pub warnings: Vec<String>,
}

pub trait ContentManifest: Port + Send + Sync {
    type Error: PortError;
    type Handle: OpaqueHandle;

    fn lookup(&self, key: &ContentKey) -> Result<ContentRecord<Self::Handle>, Self::Error>;
    fn lookup_role(
        &self,
        role: SemanticRole,
    ) -> Result<Option<ContentRecord<Self::Handle>>, Self::Error>;
    fn enumerate(
        &self,
        kind: ContentKind,
    ) -> Result<Vec<ContentRecord<Self::Handle>>, Self::Error>;
    fn verify(&self, expected: Digest) -> Result<ValidationReport, Self::Error>;
}

pub trait ContentResolver: Port + Send + Sync {
    type Error: PortError;
    type Room: OpaqueHandle;
    type ObjectKind: OpaqueHandle;
    type Script: OpaqueHandle;
    type Global: OpaqueHandle;
    type RoleTarget: OpaqueHandle;

    fn resolve_room(&self, key: &ContentKey) -> Result<Self::Room, Self::Error>;
    fn describe_room(&self, room: &Self::Room) -> Result<RoomDescriptor, Self::Error>;
    fn resolve_object_kind(&self, key: &ContentKey) -> Result<Self::ObjectKind, Self::Error>;
    fn invoke_script(
        &self,
        script: &Self::Script,
        args: ScriptArgs,
    ) -> Result<ScriptResult, Self::Error>;
    fn read_global(&self, slot: &Self::Global) -> Result<NormalizedValue, Self::Error>;
    fn write_global(
        &self,
        slot: &Self::Global,
        value: NormalizedValue,
    ) -> Result<(), Self::Error>;
    fn resolve_role(
        &self,
        role: SemanticRole,
    ) -> Result<Option<Self::RoleTarget>, Self::Error>;
    fn content_string(
        &self,
        key: &ContentKey,
        locale: &str,
    ) -> Result<ContentText, Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FormatFamily(pub [u8; 8]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FormatVersion(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FieldId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DisplayKey(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct CompanionId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireType {
    Integer,
    DecimalText,
    Text,
    Boolean,
    Bytes,
    Checkpoint,
    Unknown(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatWindow {
    pub introduced: FormatVersion,
    pub removed: Option<FormatVersion>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpaqueFieldValue {
    Absent,
    Integer(i64),
    Text(Arc<str>),
    Bytes(Arc<[u8]>),
    Encoded(Arc<[u8]>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerEntry {
    pub id: FieldId,
    pub wire: WireType,
    pub scale: Option<u32>,
    pub window: FormatWindow,
    pub original: OpaqueFieldValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotDisplay {
    pub title: Arc<str>,
    pub subtitle: Option<Arc<str>>,
    pub counters: BTreeMap<DisplayKey, i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotSummary {
    pub slot: SaveSlot,
    pub world: WorldId,
    pub display: SlotDisplay,
    pub family: FormatFamily,
    pub version: FormatVersion,
    pub revision: Revision,
    pub compatibility: Compatibility,
    pub companions: Vec<CompanionId>,
    pub metadata_damaged: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeSave {
    pub slot: SaveSlot,
    pub family: FormatFamily,
    pub version: FormatVersion,
    pub digest: Digest,
    pub bytes: Arc<[u8]>,
    pub companions: Vec<CompanionId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NeutralSave {
    pub slot: SaveSlot,
    pub world: WorldId,
    pub family: FormatFamily,
    pub version: FormatVersion,
    pub revision: Revision,
    pub display: SlotDisplay,
    pub typed: BTreeMap<FieldId, NormalizedValue>,
    pub ledger: Vec<LedgerEntry>,
    pub companions: Vec<CompanionId>,
    pub original: Option<Arc<[u8]>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveContext {
    pub slot: SaveSlot,
    pub family: Option<FormatFamily>,
    pub version: Option<FormatVersion>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedSave {
    pub family: FormatFamily,
    pub version: FormatVersion,
    pub digest: Digest,
    pub bytes: Arc<[u8]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveValidity {
    Valid,
    Invalid,
    Unsupported,
}

pub trait SaveCatalog: Port + Send + Sync {
    type Error: PortError;

    fn slots(&self) -> Result<Vec<SlotSummary>, Self::Error>;
    fn summary(&self, slot: SaveSlot) -> Result<Option<SlotSummary>, Self::Error>;
    fn authoritative(&self, slot: SaveSlot) -> Result<Option<AuthoritativeSave>, Self::Error>;
}

pub trait SaveCodec: Port + Send + Sync {
    type Error: PortError;

    fn decode(&self, bytes: &[u8], context: &SaveContext) -> Result<NeutralSave, Self::Error>;
    fn encode(&self, save: &NeutralSave) -> Result<EncodedSave, Self::Error>;
    fn validate(&self, save: &NeutralSave) -> Result<ValidationReport, Self::Error>;
    fn migrate(&self, save: NeutralSave, target: FormatVersion) -> Result<NeutralSave, Self::Error>;
}

pub trait SaveRepository: Port + Send + Sync {
    type Error: PortError;

    fn write(&self, save: &NeutralSave, target: SaveTarget) -> Result<SaveCommit, Self::Error>;
    fn delete(&self, slot: SaveSlot) -> Result<(), Self::Error>;
    fn restore_backup(&self, slot: SaveSlot) -> Result<RestoreReceipt, Self::Error>;
    fn purge_backups(&self, slot: SaveSlot) -> Result<(), Self::Error>;
    fn validate_slot(&self, slot: SaveSlot) -> Result<SaveValidity, Self::Error>;
    fn move_slot(&self, from: SaveSlot, to: SaveSlot) -> Result<(), Self::Error>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrainerCard {
    pub display_name: Arc<str>,
    pub identity: Arc<str>,
    pub phrase: Arc<str>,
    pub custom_character: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InfoValue {
    Numeric(i64),
    Text(Arc<str>),
    Digits(Arc<str>),
    Opaque(Arc<[u8]>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InfoField {
    pub index: u16,
    pub value: InfoValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartyRecord {
    pub species: u32,
    pub level: u16,
    pub experience: u32,
    pub sex: u8,
    pub iv: u32,
    pub held_item: u32,
    pub ability: u16,
    pub moves: [u16; 4],
    pub info: Vec<InfoField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NeutralTeam {
    pub trainer: TrainerCard,
    pub party: [Option<PartyRecord>; 6],
    pub version: FormatVersion,
    pub original: Arc<[u8]>,
}

pub trait TeamCodec: Port + Send + Sync {
    type Error: PortError;

    fn decode(&self, bytes: &[u8]) -> Result<NeutralTeam, Self::Error>;
    fn validate(&self, team: &NeutralTeam) -> Result<ValidationReport, Self::Error>;
    fn encode(&self, team: &NeutralTeam) -> Result<Arc<[u8]>, Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct OnlineProfileId(pub [u8; 8]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct OnlineSessionId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct OnlinePlayerId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnlineSide {
    Left,
    Right,
    Observer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnlineRequestKind {
    Fight,
    Trade,
    Gift,
    Other(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpriteState {
    pub sprite: u32,
    pub frame: u32,
    pub dx: i32,
    pub dy: i32,
    pub xscale: i32,
    pub yscale: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompanionState {
    pub species: u32,
    pub direction: u16,
    pub frame: u32,
    pub dx: i32,
    pub dy: i32,
    pub xscale: i32,
    pub yscale: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OnlinePart {
    Player(SpriteState),
    Companion(CompanionState),
    Battle,
    Custom { kind: u32, payload: Vec<u8> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnlinePosition {
    pub room: ContentKey,
    pub x: i32,
    pub y: i32,
    pub parts: Vec<OnlinePart>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerSummary {
    pub name: Arc<str>,
    pub place: Arc<str>,
    pub flags: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OnlineEvent {
    Connecting,
    Waiting,
    Started { side: OnlineSide, peer: PeerId },
    Joined { side: OnlineSide },
    Full,
    Position { player: OnlinePlayerId, position: OnlinePosition },
    PlayerLeft(OnlinePlayerId),
    Request { id: u64, from: OnlinePlayerId, kind: OnlineRequestKind, payload: Arc<[u8]> },
    Response { id: u64, from: OnlinePlayerId, accepted: bool },
    Cancelled { id: Option<u64> },
    Chat { from: OnlinePlayerId, text: Arc<str> },
    Emote { from: OnlinePlayerId, value: u32 },
    PlayerList(Vec<PlayerSummary>),
    PeerPayload { from: OnlinePlayerId, payload: Arc<[u8]> },
    BattleInput { from: OnlinePlayerId, epoch: u64, frame: u64, mask: Arc<[u8]> },
    BattleChecksum { from: OnlinePlayerId, epoch: u64, frame: u64, checksum: i64 },
    Malformed,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnlineConnectRequest {
    pub profile: OnlineProfileId,
    pub display_name: Arc<str>,
    pub room: Arc<str>,
}

pub trait OnlineSession: Send {
    type Error: PortError;

    fn session(&self) -> OnlineSessionId;
    fn send(&mut self, payload: Arc<[u8]>, wait: Wait) -> Result<SendOutcome, Self::Error>;
    fn poll(&mut self, wait: Wait) -> Result<Option<OnlineEvent>, Self::Error>;
    fn close(&mut self) -> Result<(), Self::Error>;
}

pub trait OnlineTransport: Port + Send + Sync {
    type Error: PortError;
    type Session: OnlineSession<Error = Self::Error>;

    fn connect(
        &self,
        request: OnlineConnectRequest,
        wait: Wait,
    ) -> Result<Self::Session, Self::Error>;
    fn accept(&self, wait: Wait) -> Result<Option<Self::Session>, Self::Error>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityRecord {
    pub save_families: Vec<FormatFamily>,
    pub save_versions: Vec<FormatVersion>,
    pub online_profiles: Vec<OnlineProfileId>,
    pub legacy_export: bool,
    pub companions_required: bool,
}

pub trait GameProfile: Port + Send + Sync {
    type Error: PortError;

    fn capabilities(&self) -> CapabilityRecord;
    fn map_asset(&self, key: &ContentKey) -> Result<AssetHandle, Self::Error>;
}

pub trait Clock: Send + Sync {
    type Error: PortError;

    fn wall_now(&self) -> Result<Timestamp, Self::Error>;
    fn monotonic_now(&self) -> Result<Duration, Self::Error>;
}

pub trait Entropy: Send + Sync {
    type Error: PortError;

    fn fill(&self, out: &mut [u8]) -> Result<(), Self::Error>;
}
