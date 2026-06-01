use rs_datastruct::LinkList;
use rs_vm::ScriptError;

#[repr(u8)]
pub enum CamKind {
    MoveTo = 0,
    LookAt = 1,
}

pub struct CamInfo {
    pub kind: CamKind,
    pub x: u16,
    pub z: u16,
    pub height: u16,
    pub rate: u8,
    pub rate2: u8,
}

pub struct CamQueue {
    pub queue: LinkList<CamInfo>,
}

impl CamQueue {
    pub fn new() -> Self {
        CamQueue {
            queue: LinkList::new(),
        }
    }

    pub fn add(
        &mut self,
        kind: CamKind,
        x: u16,
        z: u16,
        height: u16,
        rate: u8,
        rate2: u8,
    ) -> Result<(), ScriptError> {
        self.queue.add_tail(CamInfo {
            kind,
            x,
            z,
            height,
            rate,
            rate2,
        });
        Ok(())
    }
}
