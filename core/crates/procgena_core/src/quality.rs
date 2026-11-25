//! One quality setting that scales every asset's drawing distances together.

/// How far detail reaches, from [`Low`](Self::Low) for weak machines to
/// [`Ultra`](Self::Ultra) for strong ones.
///
/// Every [`DistanceChain`](crate::chain::DistanceChain) and
/// [`Lod`](crate::Lod) is written for [`High`](Self::High). The other tiers
/// multiply all of its distances, the cull included, by
/// [`distance_scale`](Self::distance_scale), so every asset draws nearer or
/// further together. For a chain this divides each size-on-screen
/// threshold by the same factor: at [`Low`](Self::Low) a copy switches to
/// its lower mesh while 2.5 times larger on screen than at `High`. The
/// quality setting never changes which specimen a copy draws, nor its game
/// level, only how far each step reaches.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    /// Shortest distances, for integrated GPUs and handhelds.
    Low,
    /// Shorter distances, for mid-range GPUs.
    Medium,
    /// The distances every level of detail is written for.
    #[default]
    High,
    /// Longest distances, for fast GPUs.
    Ultra,
}

impl Quality {
    /// Every tier, lowest first.
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Ultra];

    /// The factor every drawing distance is multiplied by at this tier:
    /// 0.4, 0.7, 1 and 1.5 from [`Low`](Self::Low) to [`Ultra`](Self::Ultra).
    pub const fn distance_scale(self) -> f32 {
        match self {
            Self::Low => 0.4,
            Self::Medium => 0.7,
            Self::High => 1.0,
            Self::Ultra => 1.5,
        }
    }

    /// The tier's name for menus: "Low", "Medium", "High" or "Ultra".
    pub const fn name(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Ultra => "Ultra",
        }
    }
}
