/// Every report is a 65-byte HID feature report (report id byte included).
pub const REPORT_LEN: usize = 65;

pub type Report = [u8; REPORT_LEN];

pub type TransportError = Box<dyn std::error::Error + Send + Sync>;

/// The HID feature-report channel to one mouse.
pub trait Transport {
    fn send(&self, report: &Report) -> Result<(), TransportError>;
    fn receive(&self, report: &mut Report) -> Result<(), TransportError>;
}

impl<T: Transport + ?Sized> Transport for &T {
    fn send(&self, report: &Report) -> Result<(), TransportError> {
        (**self).send(report)
    }

    fn receive(&self, report: &mut Report) -> Result<(), TransportError> {
        (**self).receive(report)
    }
}

#[cfg(feature = "hidapi")]
impl Transport for hidapi::HidDevice {
    fn send(&self, report: &Report) -> Result<(), TransportError> {
        self.send_feature_report(report)
            .map_err(|error| Box::new(error) as TransportError)
    }

    fn receive(&self, report: &mut Report) -> Result<(), TransportError> {
        self.get_feature_report(report)
            .map_err(|error| Box::new(error) as TransportError)
            .map(|_| ())
    }
}
