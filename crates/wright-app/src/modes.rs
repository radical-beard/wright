//! Editor modes — all four are live. New resource types get a new variant
//! here, a mode module, and an arm in `WrightApp::ui`.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModeId {
    Island,
    Animation,
    Dungeon,
    Placement,
}

impl ModeId {
    pub const ALL: [ModeId; 4] = [
        ModeId::Island,
        ModeId::Animation,
        ModeId::Dungeon,
        ModeId::Placement,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ModeId::Island => "🏝 Island",
            ModeId::Animation => "🦴 Animation",
            ModeId::Dungeon => "🏰 Dungeon",
            ModeId::Placement => "📍 Placement",
        }
    }
}
