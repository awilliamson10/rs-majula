use rs_io::Packet;
use rustc_hash::FxHashMap;

pub trait CacheType: Sized {
    type Context;

    fn new(id: u16) -> Self;
    fn decode(&mut self, buf: &mut Packet);
    fn post_decode(_types: &mut Vec<Self>, _ctx: &Self::Context) {}
    fn debugname(&self) -> Option<&str>;
}

pub struct TypeProvider<T> {
    pub debugnames: FxHashMap<Box<str>, u16>,
    pub types: Box<[T]>,
}

impl<T: CacheType> TypeProvider<T> {
    pub fn from_bytes(dat: &[u8], ctx: T::Context) -> TypeProvider<T> {
        let mut dat = Packet::from(dat.to_vec());

        let count = dat.g2() as usize;

        let mut debugnames = FxHashMap::with_capacity_and_hasher(count, Default::default());
        let mut types = Vec::with_capacity(count);

        for index in 0..count {
            let id = index as u16;
            let mut entry = T::new(id);
            entry.decode(&mut dat);

            if let Some(debugname) = entry.debugname() {
                debugnames.insert(Box::from(debugname), id);
            }

            types.push(entry);
        }

        T::post_decode(&mut types, &ctx);

        TypeProvider {
            debugnames,
            types: Box::from(types),
        }
    }

    pub fn get_by_id(&self, id: u16) -> Option<&T> {
        self.types.get(id as usize)
    }

    pub fn get_by_debugname(&self, name: &str) -> Option<&T> {
        self.debugnames.get(name).and_then(|&id| self.get_by_id(id))
    }

    pub fn count(&self) -> usize {
        self.types.len()
    }
}
