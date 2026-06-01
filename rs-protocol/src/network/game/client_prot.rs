use crate::network::game::client::anticheat_cyclelogic1::AnticheatCycleLogic1;
use crate::network::game::client::anticheat_cyclelogic2::AnticheatCycleLogic2;
use crate::network::game::client::anticheat_cyclelogic3::AnticheatCycleLogic3;
use crate::network::game::client::anticheat_cyclelogic4::AnticheatCycleLogic4;
use crate::network::game::client::anticheat_cyclelogic5::AnticheatCycleLogic5;
use crate::network::game::client::anticheat_cyclelogic6::AnticheatCycleLogic6;
use crate::network::game::client::anticheat_oplogic1::AnticheatOpLogic1;
use crate::network::game::client::anticheat_oplogic2::AnticheatOpLogic2;
use crate::network::game::client::anticheat_oplogic3::AnticheatOpLogic3;
use crate::network::game::client::anticheat_oplogic4::AnticheatOpLogic4;
use crate::network::game::client::anticheat_oplogic5::AnticheatOpLogic5;
use crate::network::game::client::anticheat_oplogic6::AnticheatOpLogic6;
use crate::network::game::client::anticheat_oplogic7::AnticheatOpLogic7;
use crate::network::game::client::anticheat_oplogic8::AnticheatOpLogic8;
use crate::network::game::client::anticheat_oplogic9::AnticheatOpLogic9;
use crate::network::game::client::chat_setmode::ChatSetMode;
use crate::network::game::client::client_cheat::ClientCheat;
use crate::network::game::client::close_modal::CloseModal;
use crate::network::game::client::event_camera_position::EventCameraPosition;
use crate::network::game::client::event_tracking::EventTracking;
use crate::network::game::client::friendlist_add::FriendListAdd;
use crate::network::game::client::friendlist_del::FriendListDel;
use crate::network::game::client::idk_savedesign::IdkSaveDesign;
use crate::network::game::client::idle_timer::IdleTimer;
use crate::network::game::client::if_button::IfButton;
use crate::network::game::client::ignorelist_add::IgnoreListAdd;
use crate::network::game::client::ignorelist_del::IgnoreListDel;
use crate::network::game::client::inv_button1::InvButton1;
use crate::network::game::client::inv_button2::InvButton2;
use crate::network::game::client::inv_button3::InvButton3;
use crate::network::game::client::inv_button4::InvButton4;
use crate::network::game::client::inv_button5::InvButton5;
use crate::network::game::client::inv_buttond::InvButtonD;
use crate::network::game::client::message_private::MessagePrivate;
use crate::network::game::client::message_public::MessagePublic;
use crate::network::game::client::move_gameclick::MoveGameClick;
use crate::network::game::client::move_minimapclick::MoveMinimapClick;
use crate::network::game::client::move_opclick::MoveOpClick;
use crate::network::game::client::no_timeout::NoTimeout;
use crate::network::game::client::opheld1::OpHeld1;
use crate::network::game::client::opheld2::OpHeld2;
use crate::network::game::client::opheld3::OpHeld3;
use crate::network::game::client::opheld4::OpHeld4;
use crate::network::game::client::opheld5::OpHeld5;
use crate::network::game::client::opheldt::OpHeldT;
use crate::network::game::client::opheldu::OpHeldU;
use crate::network::game::client::oploc1::OpLoc1;
use crate::network::game::client::oploc2::OpLoc2;
use crate::network::game::client::oploc3::OpLoc3;
use crate::network::game::client::oploc4::OpLoc4;
use crate::network::game::client::oploc5::OpLoc5;
use crate::network::game::client::oploct::OpLocT;
use crate::network::game::client::oplocu::OpLocU;
use crate::network::game::client::opnpc1::OpNpc1;
use crate::network::game::client::opnpc2::OpNpc2;
use crate::network::game::client::opnpc3::OpNpc3;
use crate::network::game::client::opnpc4::OpNpc4;
use crate::network::game::client::opnpc5::OpNpc5;
use crate::network::game::client::opnpct::OpNpcT;
use crate::network::game::client::opnpcu::OpNpcU;
use crate::network::game::client::opobj1::OpObj1;
use crate::network::game::client::opobj2::OpObj2;
use crate::network::game::client::opobj3::OpObj3;
use crate::network::game::client::opobj4::OpObj4;
use crate::network::game::client::opobj5::OpObj5;
use crate::network::game::client::opobjt::OpObjT;
use crate::network::game::client::opobju::OpObjU;
use crate::network::game::client::opplayer1::OpPlayer1;
use crate::network::game::client::opplayer2::OpPlayer2;
use crate::network::game::client::opplayer3::OpPlayer3;
use crate::network::game::client::opplayer4::OpPlayer4;
use crate::network::game::client::opplayert::OpPlayerT;
use crate::network::game::client::opplayeru::OpPlayerU;
use crate::network::game::client::rebuild_get_maps::RebuildGetMaps;
use crate::network::game::client::resume_p_countdialog::ResumePCountDialog;
use crate::network::game::client::resume_pause_button::ResumePauseButton;
use crate::network::game::client::send_snapshot::SendSnapshot;
use crate::network::game::client::tut_clickside::TutClickSide;
use crate::network::game::client_prot_category::ClientProtCategory;
use crate::network::game::client_prot_message::ClientProtMessageInfo;
use rs_io::PacketFrame;

macro_rules! client_prot {
    ( $( $variant:ident = $id:expr ),* $(,)? ) => {
        #[repr(u8)]
        #[derive(Debug)]
        pub enum ClientProt {
            $( $variant = $id, )*
        }

        impl TryFrom<u8> for ClientProt {
            type Error = ();

            fn try_from(value: u8) -> Result<Self, Self::Error> {
                match value {
                    $( $id => Ok(ClientProt::$variant), )*
                    _ => Err(()),
                }
            }
        }

        impl ClientProt {
            pub fn info(&self) -> ClientProtInfo {
                match self {
                    $( ClientProt::$variant => ClientProtInfo {
                        frame: <$variant>::FRAME,
                        category: <$variant>::CATEGORY,
                    }, )*
                }
            }
        }
    };
}

client_prot! {
    AnticheatOpLogic8 = 2,
    ClientCheat = 4, // NXT naming
    InvButton5 = 6, // NXT has "IF_BUTTON5" but for our interface system, this makes more sense
    AnticheatOpLogic1 = 7,
    OpNpc2 = 8, // NXT naming
    OpLocT = 9, // NXT naming
    FriendListDel = 11, // NXT naming
    AnticheatOpLogic7 = 17,
    OpNpc3 = 27, // NXT naming
    AnticheatOpLogic3 = 30,
    InvButton1 = 31, // NXT has "IF_BUTTON1" but for our interface system, this makes more sense
    InvButton4 = 38, // NXT has "IF_BUTTON4" but for our interface system, this makes more sense
    OpObj2 = 40, // NXT naming
    OpHeldT = 48, // name based on runescript trigger
    IdkSaveDesign = 52,
    OpPlayer2 = 53,
    InvButton2 = 59, // NXT has "IF_BUTTON2" but for our interface system, this makes more sense
    AnticheatOpLogic6 = 66,
    IdleTimer = 70,
    OpHeld2 = 71, // name based on runescript trigger
    OpLocU = 75, // NXT naming
    IgnoreListAdd = 79, // NXT naming
    EventTracking = 81,
    AnticheatCycleLogic5 = 85,
    AnticheatOpLogic2 = 88,
    MoveOpClick = 93, // comes with OP packets, name based on other MOVE packets
    OpLoc3 = 96, // NXT naming
    OpLoc4 = 97, // NXT naming
    OpNpc5 = 100, // NXT naming
    NoTimeout = 108, // NXT naming
    OpNpc4 = 113, // NXT naming
    OpLoc5 = 116, // NXT naming
    FriendListAdd = 118, // NXT naming
    OpHeldU = 130, // name based on runescript trigger
    OpHeld3 = 133, // name based on runescript trigger
    OpNpcT = 134, // NXT naming
    OpObjT = 138, // NXT naming
    OpObj1 = 140, // NXT naming
    AnticheatCycleLogic2 = 146,
    MessagePrivate = 148, // NXT naming
    RebuildGetMaps = 150,
    IfButton = 155, // NXT naming
    OpHeld4 = 157, // name based on runescript trigger
    MessagePublic = 158, // NXT naming
    InvButtonD = 159, // NXT has "IF_BUTTOND" but for our interface system, this makes more sense
    OpPlayer1 = 164, // NXT naming
    MoveMinimapClick = 165, // NXT naming
    IgnoreListDel = 171, // NXT naming
    OpLoc2 = 172, // NXT naming
    TutClickSide = 175,
    AnticheatOpLogic4 = 176,
    OpPlayerT = 177, // NXT naming
    OpObj4 = 178, // NXT naming
    MoveGameClick = 181, // NXT naming
    OpPlayer3 = 185, // NXT naming
    EventCameraPosition = 189, // NXT naming
    SendSnapshot = 190,
    OpNpc1 = 194, // NXT naming
    OpHeld1 = 195, // name based on runescript trigger
    OpObj3 = 200, // NXT naming
    OpNpcU = 202, // NXT naming
    OpPlayer4 = 206, // NXT naming
    OpHeld5 = 211, // name based on runescript trigger
    InvButton3 = 212, // NXT has "IF_BUTTON3" but for our interface system, this makes more sense
    AnticheatCycleLogic3 = 215,
    AnticheatCycleLogic6 = 219,
    AnticheatOpLogic5 = 220,
    CloseModal = 231, // NXT naming
    AnticheatCycleLogic1 = 233,
    ResumePauseButton = 235, // NXT naming
    AnticheatCycleLogic4 = 236,
    ResumePCountDialog = 237, // NXT naming
    AnticheatOpLogic9 = 238,
    OpObjU = 239, // NXT naming
    ChatSetMode = 244, // NXT naming
    OpLoc1 = 245, // NXT naming
    OpObj5 = 247, // NXT naming
    OpPlayerU = 248, // NXT naming
}

pub struct ClientProtInfo {
    pub frame: (PacketFrame, Option<u8>),
    pub category: ClientProtCategory,
}
