use std::convert::Infallible;
use postcard_rpc::{
    host_client::{HostClient, HostErr},
    standard_icd::{WireError, ERROR_PATH},
};
use workbook_icd::PingEndpoint;

pub struct WorkbookClient {
    pub client: HostClient<WireError>,
}

#[derive(Debug)]
pub enum WorkbookError<E> {
    Comms(HostErr<WireError>),
    Endpoint(E),
}

impl<E> From<HostErr<WireError>> for WorkbookError<E> {
    fn from(value: HostErr<WireError>) -> Self {
        Self::Comms(value)
    }
}

trait FlattenErr {
    type Good;
    type Bad;
    fn flatten(self) -> Result<Self::Good, WorkbookError<Self::Bad>>;
}

impl<T, E> FlattenErr for Result<T, E> {
    type Good = T;
    type Bad = E;
    fn flatten(self) -> Result<Self::Good, WorkbookError<Self::Bad>> {
        self.map_err(WorkbookError::Endpoint)
    }
}

// ---

impl WorkbookClient {
    pub fn new() -> Self {
        let client =
            HostClient::new_raw_nusb(|d| d.product_string() == Some("ov-twin"), ERROR_PATH, 8);
        Self { client }
    }

    pub async fn ping(&self, id: u32) -> Result<u32, WorkbookError<Infallible>> {
        let val = self.client.send_resp::<PingEndpoint>(&id).await?;
        Ok(val)
    }
}

impl Default for WorkbookClient {
    fn default() -> Self {
        Self::new()
    }
}
