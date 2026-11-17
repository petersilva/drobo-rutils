#[cfg(test)]
mod tests {
    use crate::{led_status, unit_status, Drobo};

    #[test]
    fn test_simulation_drobo() {
        let mut drobo = Drobo::new(vec!["/dev/simulated".to_string()], true).unwrap();
        assert_eq!(drobo.slot_count, 8);

        let cfg = drobo.get_subpage_config().unwrap();
        assert_eq!(cfg.max_slots, 8);

        let cap = drobo.get_subpage_capacity().unwrap();
        assert!(cap.virtual_bytes > 0);

        let slots = drobo.get_subpage_slot_info().unwrap();
        assert_eq!(slots.len(), 4);
        assert_eq!(slots[0].leds, "green");

        assert!(drobo.blink().is_ok());
        assert!(drobo.standby().is_ok());
        assert!(drobo.set_lun_size(2).is_ok());
        assert!(drobo.sync_time().is_ok());

        let mut opts = drobo.get_options().unwrap();
        opts.yellow_threshold = 80;
        opts.ip_address = "192.168.1.100".to_string();
        assert!(drobo.set_options(&opts).is_ok());
    }

    #[test]
    fn test_status_decoding() {
        let leds = led_status(3);
        assert_eq!(leds, "green");

        let status_flags = unit_status(0x0006);
        assert!(status_flags.contains(&"Red alert".to_string()));
        assert!(status_flags.contains(&"Yellow alert".to_string()));
    }
}
