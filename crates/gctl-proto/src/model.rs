pub const VENDOR_ID: u16 = 0x258A;

/// The HID interface that carries the configuration reports.
pub const INTERFACE: i32 = 0x02;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Wired,
    Wireless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    pub product_id: u16,
    pub name: &'static str,
    pub link: Link,
    /// Verified on real hardware; every other model ships untested.
    pub tested: bool,
}

/// Every product id mxw knows, wired and wireless.
pub const MODELS: [Model; 12] = [
    Model {
        product_id: 0x2022,
        name: "Model O Wireless",
        link: Link::Wireless,
        tested: true,
    },
    Model {
        product_id: 0x2011,
        name: "Model O Wireless",
        link: Link::Wired,
        tested: true,
    },
    Model {
        product_id: 0x2013,
        name: "Model O- Wireless",
        link: Link::Wireless,
        tested: false,
    },
    Model {
        product_id: 0x2024,
        name: "Model O- Wireless",
        link: Link::Wired,
        tested: false,
    },
    Model {
        product_id: 0x2027,
        name: "Model O PRO",
        link: Link::Wireless,
        tested: false,
    },
    Model {
        product_id: 0x2015,
        name: "Model O PRO",
        link: Link::Wired,
        tested: false,
    },
    Model {
        product_id: 0x2023,
        name: "Model D Wireless",
        link: Link::Wireless,
        tested: false,
    },
    Model {
        product_id: 0x2012,
        name: "Model D Wireless",
        link: Link::Wired,
        tested: false,
    },
    Model {
        product_id: 0x2025,
        name: "Model D- Wireless",
        link: Link::Wireless,
        tested: false,
    },
    Model {
        product_id: 0x2034,
        name: "Model D2 PRO",
        link: Link::Wireless,
        tested: false,
    },
    Model {
        product_id: 0x2018,
        name: "Series 1 PRO",
        link: Link::Wireless,
        tested: false,
    },
    Model {
        product_id: 0x2031,
        name: "Unknown wired model (0x2031)",
        link: Link::Wired,
        tested: false,
    },
];

impl Model {
    /// Looks a product id up in [`MODELS`].
    #[must_use]
    pub fn from_pid(product_id: u16) -> Option<&'static Self> {
        MODELS.iter().find(|model| model.product_id == product_id)
    }
}

/// One enumerated HID interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub vendor_id: u16,
    pub product_id: u16,
    pub interface: i32,
}

/// Picks the configuration interface of a known mouse, preferring the wired link:
/// a wireless mouse on its cable enumerates both product ids.
#[must_use]
pub fn select(candidates: &[Candidate]) -> Option<Candidate> {
    candidates
        .iter()
        .filter(|candidate| candidate.vendor_id == VENDOR_ID && candidate.interface == INTERFACE)
        .filter_map(|candidate| {
            Model::from_pid(candidate.product_id).map(|model| (*candidate, model.link))
        })
        .min_by_key(|(_, link)| *link == Link::Wireless)
        .map(|(candidate, _)| candidate)
}
