use rs_io::jag::JagFile;

pub struct SeqFrameProvider {
    pub delays: Box<[u8]>,
}

impl SeqFrameProvider {
    pub fn from_jag(jag_bytes: &[u8]) -> Self {
        let jag = JagFile::from(jag_bytes.to_vec());
        let delays = match jag.read("frame_del.dat") {
            Some(packet) => packet.data.into_boxed_slice(),
            None => Box::new([]),
        };
        SeqFrameProvider { delays }
    }

    pub fn get_delay(&self, frame_id: u16) -> u8 {
        self.delays.get(frame_id as usize).copied().unwrap_or(0)
    }

    pub fn count(&self) -> usize {
        self.delays.len()
    }
}
