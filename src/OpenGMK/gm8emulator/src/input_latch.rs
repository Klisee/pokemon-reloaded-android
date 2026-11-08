//! Host-side latch that swallows the gesture that closes a native overlay.
//!
//! Native menus are driven by the engine while the game underneath is still
//! running. A press that dismisses the overlay must not become a second click
//! in the game's menu on the next frame. The latch consumes that source until
//! its physical release; it deliberately lives outside the GML runtime.

#[derive(Clone, Debug, Default)]
pub struct InputLatch {
    mouse: bool,
    key: bool,
}

impl InputLatch {
    pub fn arm_mouse(&mut self) {
        self.mouse = true;
    }

    pub fn arm_key(&mut self) {
        self.key = true;
    }

    pub fn clear(&mut self) {
        self.mouse = false;
        self.key = false;
    }

    pub fn is_armed(&self) -> bool {
        self.mouse || self.key
    }

    /// Returns true when the event must be dropped. A release disarms the
    /// mouse latch even though that release is itself swallowed.
    pub fn swallow_mouse(&mut self, pressed: bool) -> bool {
        if !self.mouse {
            return false;
        }
        if !pressed {
            self.mouse = false;
        }
        true
    }

    pub fn swallow_mouse_move(&self) -> bool {
        self.mouse
    }

    /// Returns true when the event must be dropped. A key-up disarms the key
    /// latch even though that key-up is itself swallowed.
    pub fn swallow_key(&mut self, pressed: bool) -> bool {
        if !self.key {
            return false;
        }
        if !pressed {
            self.key = false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_latch_swallows_until_release_then_allows_next_click() {
        let mut latch = InputLatch::default();
        latch.arm_mouse();
        assert!(latch.swallow_mouse_move());
        assert!(latch.swallow_mouse(true));
        assert!(latch.swallow_mouse_move());
        assert!(latch.swallow_mouse(false));
        assert!(!latch.swallow_mouse_move());
        assert!(!latch.swallow_mouse(true));
        assert!(!latch.is_armed());
    }

    #[test]
    fn key_latch_swallows_until_release_and_does_not_touch_mouse() {
        let mut latch = InputLatch::default();
        latch.arm_key();
        assert!(latch.swallow_key(true));
        assert!(!latch.swallow_mouse(true));
        assert!(latch.swallow_key(false));
        assert!(!latch.swallow_key(true));
        assert!(!latch.is_armed());
    }

    #[test]
    fn clear_disarms_both_sources() {
        let mut latch = InputLatch::default();
        latch.arm_mouse();
        latch.arm_key();
        latch.clear();
        assert!(!latch.swallow_mouse(true));
        assert!(!latch.swallow_key(true));
    }
}
