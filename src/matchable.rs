use serde::{Deserialize, Serialize};

/// A trait for matching structs from "partial" filters.
///
/// This is meant as a very basic generic filter for a bunch of different structs or enums.
/// Structs are compared field by field. The "pattern" is a struct/enum with the same fields or
/// enums, but everything made optional.
pub trait PartialMatchable {
    /// The pattern type for comparison to Self. An all-values-optional sibling of Self.
    type Pattern;
    /// Test is self matches the given pattern.
    fn matches(&self, matcher: &Self::Pattern) -> bool;
}

#[derive(Serialize, Deserialize, Eq, Hash, Debug, Clone, Copy, PartialEq)]
pub enum BasicPatternOperator {
    Equals,
    NotEquals,
}

impl Default for BasicPatternOperator {
    fn default() -> Self {
        BasicPatternOperator::Equals
    }
}

#[derive(Serialize, Deserialize, Eq, Hash, Debug, Clone, Copy, PartialEq)]
#[serde(untagged)]
pub enum BasicPattern<T> {
    WithOperator {
        value: T,
        #[serde(default)]
        operator: BasicPatternOperator,
    },
    Default(T),
}

#[macro_export]
macro_rules! common_matchable {
    ($($ty:ty),*) => {
        $(
            impl $crate::matchable::PartialMatchable for $ty {
                type Pattern = ::core::option::Option<$crate::matchable::BasicPattern<$ty>>;
                fn matches(&self, pattern: &Self::Pattern) -> bool {
                    let (value, operator) = match pattern {
                        Some($crate::matchable::BasicPattern::Default(v)) => (v, &$crate::matchable::BasicPatternOperator::Equals),
                        Some($crate::matchable::BasicPattern::WithOperator{ value: v, operator: o}) => (v, o),
                        None => return true,
                    };

                    match operator {
                        $crate::matchable::BasicPatternOperator::Equals => self == value,
                        $crate::matchable::BasicPatternOperator::NotEquals => self != value,
                    }
                }
            }
        )*
    };
}

// The generic impls for common types (used in the crate)
common_matchable![String, bool, i32, u64];

/// Implement PartialMatchable for structs and (unit) enums
///
/// Creates an "all optional" copy of the given struct or enum and implements the
/// PartialMatchable trait for the original.
///
/// # Example
///```rust
/// use drbd_reactor::make_matchable;
/// use drbd_reactor::matchable::PartialMatchable;
/// use drbd_reactor::matchable::BasicPattern;
/// use drbd_reactor::matchable::BasicPatternOperator;
///
/// make_matchable!(struct Foo { item: String }, FooPattern);
///
/// let foo_pattern = Some(FooPattern { item: Some(BasicPattern::Default("a".to_string())) });
/// let a = Foo { item: "a".to_string() };
/// let b = Foo { item: "b".to_string() };
/// assert!(a.matches(&None));
/// assert!(a.matches(&foo_pattern));
/// assert!(!b.matches(&foo_pattern));
///
/// make_matchable!(enum Bar { A, B });
/// let bar_pattern = Some(BasicPattern::Default(Bar::A));
/// let negative_pattern = Some(BasicPattern::WithOperator {value: Bar::A, operator: BasicPatternOperator::NotEquals});
/// let a = Bar::A;
/// let b = Bar::B;
/// assert!(a.matches(&None));
/// assert!(a.matches(&bar_pattern));
/// assert!(!b.matches(&bar_pattern));
/// assert!(!a.matches(&negative_pattern));
/// assert!(b.matches(&negative_pattern));
///
///```
#[macro_export]
macro_rules! make_matchable {
    ($(#[$structattr:meta])* $svis:vis struct $name:ident { $($(#[$fieldattr:meta])* $fvis:vis $field:ident: $field_ty:ty),* $(,)? }, $pattern:ident) => {
        $(
            #[$structattr]
        )*
        $svis struct $name {
        $(
            $(
                #[$fieldattr]
            )*
            $fvis $field: $field_ty,
        )*
        }

        $(
            #[$structattr]
        )*
        $svis struct $pattern {
        $(
            $(
                #[$fieldattr]
            )*
            $fvis $field: <$field_ty as $crate::matchable::PartialMatchable>::Pattern,
        )*
        }

        impl $crate::matchable::PartialMatchable for $name {
            type Pattern = ::core::option::Option<$pattern>;
            fn matches(&self, pattern: &Self::Pattern) -> bool {
                let pattern = match pattern.as_ref() {
                    Some(v) => v,
                    None => return true,
                };
                $(
                    if !$crate::matchable::PartialMatchable::matches(&self.$field, &pattern.$field) {
                        return false
                    }
                )*
                true
            }
        }
    };

    // Matches basic enums: unit variants should work, tuple and struct variant won't
    // The pattern type is Option<Enum>
    ($(#[$enumattr:meta])* $evis:vis enum $name:ident { $($(#[$variantattr:meta])* $variant:ident),* $(,)? }) => {
        $(
            #[$enumattr]
        )*
        $evis enum $name {
        $(
            $(
                #[$variantattr]
            )*
            $variant,
        )*
        }

        impl $crate::matchable::PartialMatchable for $name {
            type Pattern = ::core::option::Option<$crate::matchable::BasicPattern<$name>>;
            fn matches(&self, pattern: &Self::Pattern) -> bool {
                let (value, operator) = match pattern {
                        Some($crate::matchable::BasicPattern::Default(v)) => (v, &$crate::matchable::BasicPatternOperator::Equals),
                        Some($crate::matchable::BasicPattern::WithOperator{ value: v, operator: o}) => (v, o),
                        None => return true,
                };

                match (self, value) {
                $(
                    ($name::$variant, $name::$variant) => &$crate::matchable::BasicPatternOperator::Equals == operator,
                )*
                    _ => &$crate::matchable::BasicPatternOperator::NotEquals == operator,
                }
            }
        }
    };
}
