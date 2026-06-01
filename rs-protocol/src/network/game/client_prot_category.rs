#[repr(u8)]
pub enum ClientProtCategory {
    ClientEvent = 20,
    UserEvent = 5,
    RestrictedEvent = 2,
}
