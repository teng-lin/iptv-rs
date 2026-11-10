//! Where a value applies: the declared override depth of a setting
//! ([`Scope`]) and the address of a stored value ([`Target`]).
//!
//! Resolution walks from the most specific level to the least:
//! group → playlist → global → schema default. A level a setting's scope
//! does not allow is skipped (and cannot be written).

/// The deepest level at which a setting may be overridden.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// One value for the whole app.
    Global,
    /// A global value that each playlist may override.
    Playlist,
    /// A global value that each playlist and each group may override.
    Group,
}

impl Scope {
    pub const fn key(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Playlist => "playlist",
            Scope::Group => "group",
        }
    }

    /// Whether a value may be stored at `target`.
    pub fn allows(self, target: &Target) -> bool {
        match target {
            Target::Global => true,
            Target::Playlist(_) => self >= Scope::Playlist,
            Target::Group(..) => self >= Scope::Group,
        }
    }
}

/// The address of a stored value. Ids are the stable content ids from the
/// provider layer; a group id is only unique within its playlist.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    Global,
    Playlist(String),
    /// `(playlist id, group id)`.
    Group(String, String),
}

impl Target {
    pub fn playlist(id: impl Into<String>) -> Target {
        Target::Playlist(id.into())
    }

    pub fn group(playlist: impl Into<String>, group: impl Into<String>) -> Target {
        Target::Group(playlist.into(), group.into())
    }

    /// The levels consulted when resolving at `self`, most specific first.
    pub fn chain(&self) -> Vec<Target> {
        match self {
            Target::Global => vec![Target::Global],
            Target::Playlist(_) => vec![self.clone(), Target::Global],
            Target::Group(p, _) => vec![self.clone(), Target::Playlist(p.clone()), Target::Global],
        }
    }

    pub const fn scope(&self) -> Scope {
        match self {
            Target::Global => Scope::Global,
            Target::Playlist(_) => Scope::Playlist,
            Target::Group(..) => Scope::Group,
        }
    }
}

impl core::fmt::Display for Target {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Target::Global => f.write_str("global"),
            Target::Playlist(p) => write!(f, "playlist {p:?}"),
            Target::Group(p, g) => write!(f, "group {g:?} of playlist {p:?}"),
        }
    }
}
