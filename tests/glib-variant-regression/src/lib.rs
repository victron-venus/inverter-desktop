#[cfg(test)]
mod tests {
    use glib::prelude::*;

    #[test]
    fn strings_are_returned_after_native_out_pointer_write() {
        let source = ["first", "пароль", "🔒", "last"];
        let variant = source.to_variant();
        assert_eq!(
            variant.array_iter_str().unwrap().collect::<Vec<_>>(),
            source
        );
        assert_eq!(
            variant.array_iter_str().unwrap().rev().collect::<Vec<_>>(),
            ["last", "🔒", "пароль", "first"]
        );
    }

    #[test]
    fn indexed_and_last_access_use_the_same_valid_pointer() {
        let variant = ["zero", "one", "two", "three"].to_variant();
        assert_eq!(variant.array_iter_str().unwrap().nth(1), Some("one"));
        assert_eq!(variant.array_iter_str().unwrap().nth_back(1), Some("two"));
        assert_eq!(variant.array_iter_str().unwrap().last(), Some("three"));
    }

    #[test]
    fn mixed_direction_and_exhaustion_stay_bounded() {
        let variant = ["a", "b", "c"].to_variant();
        let mut iter = variant.array_iter_str().unwrap();
        assert_eq!(iter.next(), Some("a"));
        assert_eq!(iter.next_back(), Some("c"));
        assert_eq!(iter.next(), Some("b"));
        assert_eq!(iter.next_back(), None);
        assert_eq!(iter.next(), None);
        assert_eq!(iter.len(), 0);
        let empty = Vec::<String>::new().to_variant();
        assert_eq!(empty.array_iter_str().unwrap().next(), None);
        assert_eq!(variant.array_iter_str().unwrap().nth(usize::MAX), None);
        assert_eq!(variant.array_iter_str().unwrap().nth_back(usize::MAX), None);
    }
}
