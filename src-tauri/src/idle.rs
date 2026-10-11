#[cfg(target_os = "macos")]
mod macos {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state_id: u32, event_type: u32) -> f64;
    }

    const HID_SYSTEM_STATE: u32 = 1;
    const ANY_INPUT: u32 = 0xFFFF_FFFF;

    pub fn seconds_since_last_input() -> f64 {
        CGEventSourceSecondsSinceLastEventType(HID_SYSTEM_STATE, ANY_INPUT)
    }
}

#[cfg(not(target_os = "macos"))]
mod macos {
    pub fn seconds_since_last_input() -> f64 {
        0.0
    }
}

pub use macos::seconds_since_last_input;
