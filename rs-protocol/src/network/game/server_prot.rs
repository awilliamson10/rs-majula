macro_rules! server_prot {
    ( $( $variant:ident = $id:expr ),* $(,)? ) => {
        #[repr(u8)]
        #[derive(Debug)]
        pub enum ServerProt {
            $( $variant = $id, )*
        }
    };
}

server_prot! {
    NpcInfo = 1, // NXT naming
    IfSetColour = 2, // NXT naming
    CamMoveTo = 3, // NXT naming
    MessageGame = 4, // NXT naming
    UpdateZonePartialFollows = 7, // NXT naming
    SynthSound = 12, // NXT naming
    CamShake = 13, // NXT naming
    IfOpenChat = 14,
    UpdateInvStopTransmit = 15, // NXT naming
    UnsetMapFlag = 19, // NXT has "SET_MAP_FLAG" but we cannot control the position
    DataLocDone = 20,
    UpdateIgnoreList = 21, // NXT naming
    UpdateRunWeight = 22, // NXT naming
    LocMerge = 23,
    HintArrow = 25, // NXT naming
    IfSetHide = 26, // NXT naming
    IfOpenMainSide = 28,
    ChatFilterSettings = 32, // NXT naming
    MessagePrivate = 41, // NXT naming
    LocAnim = 42,
    UpdateRebootTimer = 43, // NXT naming
    UpdateStat = 44, // NXT naming
    IfSetObject = 46, // NXT naming
    ObjDel = 49,
    ObjReveal = 50,
    MidiSong = 54, // NXT naming
    LocAddChange = 59,
    UpdateRunEnergy = 68, // NXT naming
    MapProjAnim = 69,
    CamLookAt = 74, // NXT naming
    LocDel = 76,
    DataLandDone = 80,
    IfSetTabActive = 84,
    IfSetModel = 87, // NXT naming
    UpdateInvFull = 98, // NXT naming
    TutFlash = 126,
    IfClose = 129,
    IfSetRecol = 103, // NXT naming
    DataLand = 132,
    FinishTracking = 133,
    UpdateZoneFullFollows = 135, // NXT naming
    ResetAnims = 136, // NXT naming
    UpdatePid = 139,
    LastLoginInfo = 140, // NXT naming
    Logout = 142, // NXT naming
    IfSetAnim = 146, // NXT naming
    VarpSmall = 150, // NXT naming
    ObjCount = 151,
    UpdateFriendList = 152, // NXT naming
    UpdateZonePartialEnclosed = 162, // NXT naming
    IfSetTab = 167,
    IfOpenMain = 168,
    VarpLarge = 175, // NXT naming
    PlayerInfo = 184, // NXT naming
    TutOpen = 185,
    MapAnim = 191,
    ResetClientVarCache = 193, // NXT naming
    IfOpenSide = 195,
    IfSetPlayerHead = 197, // NXT naming
    IfSetText = 201, // NXT naming
    IfSetNpcHead = 204, // NXT naming
    IfSetPosition = 209, // NXT naming
    MidiJingle = 212, // NXT naming
    UpdateInvPartial = 213, // NXT naming
    DataLoc = 220,
    ObjAdd = 223,
    EnableTracking = 226,
    RebuildNormal = 237, // NXT naming (do we really need _normal if there's no region rebuild?)
    CamReset = 239, // NXT naming
    PCountDialog = 243, // named after runescript command + client resume_p_countdialog packet
    SetMultiway = 254,
}
