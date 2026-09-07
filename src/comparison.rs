//! One explicit native comparison at a time; no persisted credentials or inventory.
use koi_ui::devices::{Comparison, DeviceId, Reading};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct Store(Arc<Mutex<Option<(DeviceId, Comparison)>>>);

impl Store {
    pub fn view(&self, peer: Option<&DeviceId>) -> Comparison {
        self.0
            .lock()
            .expect("comparison state")
            .as_ref()
            .filter(|(id, _)| Some(id) == peer)
            .map_or(Comparison::NotRun, |(_, state)| state.clone())
    }

    pub fn start(&self, peer: DeviceId) -> bool {
        let mut state = self.0.lock().expect("comparison state");
        if matches!(&*state, Some((_, Comparison::Comparing))) {
            return false;
        }
        *state = Some((peer.clone(), Comparison::Comparing));
        drop(state);
        let store = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let result = std::panic::catch_unwind(|| run(&peer)).unwrap_or_else(|_| {
                Comparison::Incomplete("The read was interrupted. Compare again.".into())
            });
            *store.0.lock().expect("comparison state") = Some((peer, result));
        });
        true
    }
}

fn reading(
    observer: String,
    result: Result<koi_ui::devices::MdnsDiscoverySnapshot, String>,
) -> Box<Reading> {
    Box::new(Reading {
        observer,
        received_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        result,
    })
}

fn read_error(error: koi_client::ClientError) -> String {
    match error {
        koi_client::ClientError::Unauthorized => "Access refused. The peer must permit its discovery read API; this computer’s local token cannot authorize that read.".into(),
        koi_client::ClientError::Decode(_) => "This device did not return a supported discovery snapshot. Update Koi on that device, then retry.".into(),
        _ => "Discovery could not be read. Check that this device is online and permits its advertised discovery endpoint, then retry.".into(),
    }
}

fn run(id: &DeviceId) -> Comparison {
    let Ok(client) = koi_client::KoiClient::from_local() else {
        return Comparison::Incomplete(
            "Cannot reach local Koi. Start the local service and retry.".into(),
        );
    };
    let Ok(catalog) = client.catalog_snapshot() else {
        return Comparison::Incomplete(
            "Cannot read the current device catalog. Refresh Devices and retry.".into(),
        );
    };
    let Some(peer) = koi_ui::devices::peers(&catalog)
        .into_iter()
        .find(|peer| &peer.id == id)
    else {
        return Comparison::Incomplete("The selected peer is no longer eligible. Reconnect it or choose a currently discovered device.".into());
    };
    let local_label = catalog
        .local_device_id
        .as_ref()
        .and_then(|id| catalog.devices.iter().find(|device| &device.id == id))
        .map_or("This device".to_string(), |device| {
            format!("This device ({})", koi_ui::devices::label(device))
        });
    std::thread::scope(|scope| {
        let left = scope.spawn(|| {
            reading(
                local_label,
                client.comparison_snapshot().map_err(read_error),
            )
        });
        let right = scope.spawn(|| {
            reading(
                format!("{} (advertised observer at {})", peer.label, peer.endpoint),
                koi_client::comparison::read_peer(&peer.endpoint).map_err(read_error),
            )
        });
        match (left.join(), right.join()) {
            (Ok(local), Ok(peer)) => Comparison::Finished { local, peer },
            _ => Comparison::Incomplete("A discovery read was interrupted. Compare again.".into()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_running_comparison_cannot_start_another_read_or_change_selection() {
        let id = DeviceId::new("office").unwrap();
        let store = Store(Arc::new(Mutex::new(Some((
            id.clone(),
            Comparison::Comparing,
        )))));
        assert!(!store.start(DeviceId::new("other").unwrap()));
        assert!(matches!(store.view(Some(&id)), Comparison::Comparing));
        assert!(matches!(store.view(None), Comparison::NotRun));
        assert!(matches!(
            store.view(Some(&DeviceId::new("other").unwrap())),
            Comparison::NotRun
        ));
    }
}
