//! UAC2 request types, request codes, and control selectors.

/// bmRequestType for SET_CUR: Host→Device, Class, Interface.
pub const REQ_TYPE_SET: u8 = 0x21;

/// bmRequestType for GET_CUR: Device→Host, Class, Interface.
pub const REQ_TYPE_GET: u8 = 0xA1;

/// bRequest = CUR (current value).
pub const REQ_CUR: u8 = 0x01;

/// Control selector for Feature Unit volume (FU10, FU11).
pub const CS_VOLUME: u8 = 2;

/// Control selector for Mixer Unit cross-point (MU60).
pub const CS_MIXER: u8 = 1;

/// EU58: phantom 48V power per input.
pub const CS_EU58_PHANTOM: u8 = 0;

/// EU58: input mute per channel.
pub const CS_EU58_MUTE: u8 = 2;

/// EU59: output mute.
pub const CS_EU59_MUTE: u8 = 1;
