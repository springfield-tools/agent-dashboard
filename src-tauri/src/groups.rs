pub fn group_count() -> usize {
    crate::model::GROUP_IDS.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_six_groups() {
        assert_eq!(group_count(), 6);
        assert_eq!(crate::model::GROUP_IDS[0], "needs-input");
        assert_eq!(crate::model::GROUP_IDS[5], "not-started");
    }
}
