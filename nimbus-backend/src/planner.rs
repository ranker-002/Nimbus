//! Works out *how* a hotspot would be brought up before anything is changed.
//!
//! The planner exists because starting an access point is not a harmless
//! operation: on a laptop with a single Wi-Fi radio, the adapter that would
//! host the hotspot is usually the same one carrying the machine's internet
//! connection. Bringing up an AP on it can tear that connection down. Every
//! decision that risks the uplink is made here, up front, so the UI can warn
//! before a single NetworkManager call happens.

use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::{
    freq_to_channel, is_dfs_channel, AdapterCapabilities, Band, HotspotBackend, HotspotConfig,
    HotspotPlan, InterfaceState, RegDomainChange, Security, StaImpact, StationConnection,
};
use nimbus_network::regdomain;

use crate::shared_ap::Missing;
use nimbus_network::traits::NetworkManagerApi;

/// Builds a [`HotspotPlan`] for `config`. Performs only read-only queries.
///
/// `requested_interface` pins the adapter; `None` lets the planner choose the
/// one least likely to cost the user their connection.
pub async fn plan_hotspot(
    nm: &dyn NetworkManagerApi,
    config: &HotspotConfig,
    requested_interface: Option<&str>,
) -> Result<HotspotPlan> {
    config.validate()?;

    if !nm.is_nm_available().await {
        return Err(NimbusError::NetworkManagerUnavailable(
            "NetworkManager D-Bus service not reachable".into(),
        ));
    }

    // Resolve the uplink *first*. Once an AP is up on the wrong adapter the
    // default route may already be gone, and the old code read it afterwards —
    // which is how NAT ended up pointed at a hardcoded "eth0".
    let upstream = nm.get_upstream_interface().await.unwrap_or(None);

    let ap_interface = match requested_interface {
        Some(name) => name.to_string(),
        None => choose_ap_interface(nm, upstream.as_deref()).await?,
    };

    let caps = nm.get_adapter_capabilities(&ap_interface).await?;
    check_supported(config, &caps)?;

    let station = nm
        .get_station_connection(&ap_interface)
        .await
        .unwrap_or(None);

    // When the adapter is already carrying a connection, try to keep it: the
    // shared backend puts the AP on a second interface instead of taking over
    // this one.
    let mut blockers = crate::shared_ap::check_requirements().await;
    if needs_regulatory_database(station.as_ref()).await {
        blockers.push(Missing::RegulatoryDatabase);
    }
    let (backend, channel) = choose_backend(&caps, station.as_ref(), &blockers);

    let impact = resolve_impact(backend, station.as_ref());
    let mut effective_config = config.clone();
    if let Some(channel) = channel {
        effective_config.channel = Some(channel);
        effective_config.band = Band::of_channel(channel);
    }
    // Only the NetworkManager backend is limited to one key-mgmt value;
    // hostapd can run a real transition AP.
    let security_downgraded =
        backend == HotspotBackend::NetworkManager && resolve_security(&mut effective_config);

    let regdomain_change = match &effective_config.country_code {
        Some(wanted) => resolve_regdomain(wanted, regdomain::current().await),
        None => None,
    };

    let mut warnings = build_warnings(&impact, &caps, &ap_interface, &upstream);
    if !caps.detected {
        warnings.push(format!(
            "The capabilities of {} could not be read (is the `iw` tool \
             installed?), so band and WPA3 support could not be checked before \
             starting.",
            ap_interface
        ));
    }
    warnings.extend(backend_notes(
        backend,
        &caps,
        station.as_ref(),
        &blockers,
        channel,
        effective_config.country_code.as_deref(),
    ));
    if security_downgraded {
        warnings.push(
            "NetworkManager cannot run a mixed WPA2/WPA3 access point, so the \
             hotspot will use WPA2. Choose WPA3 explicitly if every device \
             supports it."
                .into(),
        );
    }
    if let Some(change) = &regdomain_change {
        warnings.push(format!(
            "The regulatory domain will be changed from {} to {} for the whole \
             machine while the hotspot runs, and put back when it stops.",
            change.from.as_deref().unwrap_or("unset"),
            change.to,
        ));
        if station.is_some() {
            warnings.push(
                "Changing the regulatory domain can affect the Wi-Fi network \
                 you are connected to, since it applies to every adapter."
                    .into(),
            );
        }
    }

    // Only meaningful when there is a connection to preserve; otherwise the
    // shared backend was never on the table.
    let share_blockers: Vec<String> = if station.is_some() {
        blockers
            .iter()
            .map(|missing| missing.explain().to_string())
            .collect()
    } else {
        Vec::new()
    };

    Ok(HotspotPlan {
        warnings,
        ap_interface,
        upstream_interface: upstream,
        backend,
        channel,
        effective_config,
        impact,
        regdomain_change,
        share_blockers,
    })
}

/// Whether sharing would need a regulatory database the machine does not have.
///
/// The AP has to follow the station connection's channel. On the world domain
/// (`00`) every 5 GHz band is receive-only, so an access point there cannot
/// legally transmit and hostapd will refuse the channel. Escaping that domain
/// means resolving a country, which the kernel can only do with
/// `wireless-regdb` installed.
async fn needs_regulatory_database(station: Option<&StationConnection>) -> bool {
    let Some(station) = station else {
        return false;
    };
    if Band::of_channel(freq_to_channel(station.frequency)) != Band::Band5Ghz {
        return false;
    }

    let on_world_domain = regdomain::current()
        .await
        .is_none_or(|domain| domain == regdomain::WORLD);

    on_world_domain && !regdomain::database_present().await
}

/// Chooses how to create the AP, and the channel to pin it to.
///
/// The shared backend is only worth its extra requirements when there is a
/// connection to preserve; with a free adapter, NetworkManager is simpler and
/// needs no privileges.
fn choose_backend(
    caps: &AdapterCapabilities,
    station: Option<&StationConnection>,
    blockers: &[Missing],
) -> (HotspotBackend, Option<u32>) {
    let Some(station) = station else {
        return (HotspotBackend::NetworkManager, None);
    };

    if !caps.supports_simultaneous_sta_ap || !blockers.is_empty() {
        return (HotspotBackend::NetworkManager, None);
    }

    // Both interfaces share the radio. When it can only hold one channel, the
    // AP has to follow the station connection or neither will work.
    let channel = if caps.sta_ap_same_channel_only {
        let channel = freq_to_channel(station.frequency);
        if channel == 0 {
            // The station's channel is unknown, so it cannot be matched.
            return (HotspotBackend::NetworkManager, None);
        }
        Some(channel)
    } else {
        None
    };

    (HotspotBackend::SharedVirtualAp, channel)
}

/// Explains the chosen backend: why the connection is kept, or what would be
/// needed to keep it.
fn backend_notes(
    backend: HotspotBackend,
    caps: &AdapterCapabilities,
    station: Option<&StationConnection>,
    blockers: &[Missing],
    channel: Option<u32>,
    country: Option<&str>,
) -> Vec<String> {
    let mut notes = Vec::new();

    match backend {
        HotspotBackend::SharedVirtualAp => {
            notes.push(format!(
                "Your Wi-Fi connection stays up: the hotspot runs on a separate \
                 interface ({}) and shares it{}.",
                nimbus_network::virtual_ap::AP_INTERFACE,
                match channel {
                    Some(channel) => format!(", on channel {}", channel),
                    None => String::new(),
                }
            ));

            if let Some(channel) = channel.filter(|c| is_dfs_channel(*c)) {
                notes.push(format!(
                    "Channel {} is a radar-detection (DFS) channel, so the \
                     hotspot listens for weather radar before it starts. Expect \
                     it to take a minute or so.",
                    channel
                ));
                if country.is_none() {
                    notes.push(
                        "Radar-detection channels need a country to be set: \
                         without one the access point will refuse to start. Set \
                         a country code, or reconnect your Wi-Fi on a channel \
                         outside 52-64 and 100-144."
                            .into(),
                    );
                }
            }
        }
        HotspotBackend::NetworkManager => {
            // Only worth explaining when sharing was actually on the table.
            if station.is_some() && caps.supports_simultaneous_sta_ap {
                for blocker in blockers {
                    notes.push(format!(
                        "Cannot share your connection: {}",
                        blocker.explain()
                    ));
                }
            }
        }
    }

    notes
}

/// Rewrites a security setting NetworkManager cannot honour into the one it
/// will actually apply. Returns whether anything was changed.
///
/// `key-mgmt` holds a single value, so there is no way to ask for a WPA2/WPA3
/// transition AP; requesting one used to fail with `InvalidProperty` at
/// activation time. Degrading it here means the plan the user is shown is the
/// hotspot they get.
fn resolve_security(config: &mut HotspotConfig) -> bool {
    if config.security == Security::Wpa2Wpa3Transition {
        config.security = Security::Wpa2;
        return true;
    }
    false
}

/// Reports the domain switch needed to reach `wanted`, or `None` when the
/// machine is already using it.
fn resolve_regdomain(wanted: &str, current: Option<String>) -> Option<RegDomainChange> {
    let wanted = regdomain::normalize(wanted);
    if current.as_deref() == Some(wanted.as_str()) {
        return None;
    }
    Some(RegDomainChange {
        from: current,
        to: wanted,
    })
}

/// Picks the Wi-Fi adapter that costs the user the least.
///
/// Preference order: an AP-capable adapter that is not the uplink, then one
/// that can hold a station connection and an AP at once, then whatever is left.
async fn choose_ap_interface(nm: &dyn NetworkManagerApi, upstream: Option<&str>) -> Result<String> {
    let devices = nm.get_wifi_devices().await?;
    if devices.is_empty() {
        return Err(NimbusError::InterfaceNotFound(
            "no Wi-Fi adapter present".into(),
        ));
    }

    let mut best: Option<(u8, String)> = None;
    let mut saw_ap_capable = false;

    for device in &devices {
        // Never host the AP on the interface Nimbus creates for hosting APs.
        // One left behind by a crashed run otherwise looks like a spare
        // adapter and wins the selection outright.
        if device.name == nimbus_network::virtual_ap::AP_INTERFACE {
            continue;
        }
        if device.state == InterfaceState::Unavailable {
            // Rfkill'd or otherwise not usable.
            continue;
        }

        let caps = match nm.get_adapter_capabilities(&device.name).await {
            Ok(caps) => caps,
            Err(e) => {
                log::warn!("Skipping {}: {}", device.name, e);
                continue;
            }
        };
        if !caps.supports_ap && caps.detected {
            continue;
        }
        saw_ap_capable = true;

        // Using the uplink adapter always costs the connection (see
        // `resolve_impact`), so a spare adapter — even a lesser one — wins.
        let is_upstream = upstream.is_some_and(|u| u == device.name);
        let score = u8::from(!is_upstream);

        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, device.name.clone()));
        }
    }

    match best {
        Some((_, name)) => Ok(name),
        None if saw_ap_capable => Err(NimbusError::InterfaceNotFound(
            "no usable Wi-Fi adapter".into(),
        )),
        None => Err(NimbusError::ApModeNotSupported(
            devices
                .iter()
                .map(|d| d.name.clone())
                .collect::<Vec<_>>()
                .join(", "),
        )),
    }
}

fn check_supported(config: &HotspotConfig, caps: &AdapterCapabilities) -> Result<()> {
    // Without `iw` we know nothing about the radio. NetworkManager or hostapd
    // will give a precise error if the request cannot be honoured, which beats
    // refusing based on guessed capabilities.
    if !caps.detected {
        return Ok(());
    }
    if !caps.supports_ap {
        return Err(NimbusError::ApModeNotSupported(caps.interface.clone()));
    }
    if config.security == nimbus_core::types::Security::Wpa3 && !caps.supports_wpa3 {
        return Err(NimbusError::Wpa3NotSupported(caps.interface.clone()));
    }
    if config.band == Band::Band5Ghz && caps.supported_channels_5ghz.is_empty() {
        return Err(NimbusError::Band5GhzNotSupported(caps.interface.clone()));
    }
    Ok(())
}

/// Decides what happens to an existing station connection on the AP adapter.
///
/// The answer depends entirely on the backend: the shared one adds a second
/// interface and leaves the connection alone, while NetworkManager reuses the
/// device and drops it.
///
/// Verified against NetworkManager 1.54 on a single-radio laptop: activating an
/// AP profile deactivates whatever the device was doing first —
///
/// ```text
/// wlp4s0: state change: activated -> deactivating (reason 'user-requested')
/// wlp4s0: state change: deactivating -> disconnected
/// wlp4s0: Activation: starting connection 'Nimbus-NimbusTest'
/// ```
///
/// — because a NetworkManager device holds exactly one active connection. That
/// happens no matter what the radio's interface-combination table permits, so
/// the adapter's `supports_simultaneous_sta_ap` capability cannot be cashed in
/// here. Doing so would need a second virtual interface (`iw dev … interface
/// add … type __ap`) with its own AP daemon, which Nimbus does not create.
///
/// An earlier version pinned the AP to the station's channel and reported the
/// connection as safe. It was not: the pin is irrelevant when the device is
/// torn down before the AP is brought up.
fn resolve_impact(backend: HotspotBackend, station: Option<&StationConnection>) -> StaImpact {
    match (backend, station) {
        // No client connection on this adapter: nothing to lose.
        (_, None) => StaImpact::None,
        // A second interface is created; the station connection is untouched.
        (HotspotBackend::SharedVirtualAp, Some(_)) => StaImpact::None,
        (HotspotBackend::NetworkManager, Some(station)) => StaImpact::DisconnectsUplink {
            ssid: station.ssid.clone(),
        },
    }
}

fn build_warnings(
    impact: &StaImpact,
    caps: &AdapterCapabilities,
    ap_interface: &str,
    upstream: &Option<String>,
) -> Vec<String> {
    let mut warnings = Vec::new();

    if let StaImpact::DisconnectsUplink { ssid } = impact {
        let network = ssid.as_deref().unwrap_or("your Wi-Fi network");
        warnings.push(format!(
            "Starting the hotspot on {} will disconnect you from {}.",
            ap_interface, network
        ));
        if caps.supports_simultaneous_sta_ap {
            // Worth saying: the limit is the software, not their hardware.
            warnings.push(
                "This adapter's radio could run a hotspot and stay connected at \
                 the same time, but NetworkManager brings the hotspot up on the \
                 same network device, which drops the connection."
                    .into(),
            );
        }
    }

    match upstream {
        None => warnings.push(
            "No internet connection was found, so devices joining the hotspot \
             will not have internet access."
                .into(),
        ),
        Some(name) if name == ap_interface && impact.is_disconnecting() => warnings.push(
            "This adapter is your only internet connection, so devices joining \
             the hotspot will not have internet access."
                .into(),
        ),
        Some(_) => {}
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{caps, config, FakeNm};

    /// Channel 112 = 5560 MHz, the channel the reporting user's laptop was on.
    fn sta_link() -> StationConnection {
        StationConnection {
            interface: "wlan0".into(),
            ssid: Some("HomeNet".into()),
            frequency: 5560,
        }
    }

    #[test]
    fn no_station_connection_means_no_impact() {
        assert_eq!(
            resolve_impact(HotspotBackend::NetworkManager, None),
            StaImpact::None
        );
    }

    /// On the NetworkManager backend the device is reused, so the station
    /// connection goes no matter what the radio is capable of.
    #[test]
    fn networkmanager_backend_always_loses_the_station_connection() {
        assert_eq!(
            resolve_impact(HotspotBackend::NetworkManager, Some(&sta_link())),
            StaImpact::DisconnectsUplink {
                ssid: Some("HomeNet".into())
            }
        );
    }

    #[test]
    fn a_capable_radio_is_called_out_as_a_software_limit() {
        let impact = resolve_impact(HotspotBackend::NetworkManager, Some(&sta_link()));
        let warnings = build_warnings(&impact, &caps(true, true), "wlan0", &Some("wlan0".into()));
        assert!(warnings.iter().any(|w| w.contains("will disconnect you")));
        assert!(warnings.iter().any(|w| w.contains("NetworkManager")));
    }

    #[test]
    fn an_incapable_radio_gets_no_software_limit_note() {
        let impact = resolve_impact(HotspotBackend::NetworkManager, Some(&sta_link()));
        let warnings = build_warnings(&impact, &caps(false, false), "wlan0", &Some("wlan0".into()));
        assert!(warnings.iter().any(|w| w.contains("will disconnect you")));
        assert!(!warnings.iter().any(|w| w.contains("NetworkManager")));
    }

    #[test]
    fn disconnecting_plan_needs_confirmation() {
        let plan = HotspotPlan {
            ap_interface: "wlan0".into(),
            upstream_interface: Some("wlan0".into()),
            backend: HotspotBackend::NetworkManager,
            channel: None,
            effective_config: config(),
            impact: resolve_impact(HotspotBackend::NetworkManager, Some(&sta_link())),
            regdomain_change: None,
            share_blockers: Vec::new(),
            warnings: Vec::new(),
        };
        assert!(plan.needs_confirmation());
    }

    #[test]
    fn untouched_connection_needs_no_confirmation() {
        let plan = HotspotPlan {
            ap_interface: "wlan1".into(),
            upstream_interface: Some("enp5s0".into()),
            backend: HotspotBackend::NetworkManager,
            channel: None,
            effective_config: config(),
            impact: StaImpact::None,
            regdomain_change: None,
            share_blockers: Vec::new(),
            warnings: Vec::new(),
        };
        assert!(!plan.needs_confirmation());
    }

    #[test]
    fn missing_upstream_warns_about_no_internet() {
        let warnings = build_warnings(&StaImpact::None, &caps(true, true), "wlan1", &None);
        assert!(warnings
            .iter()
            .any(|w| w.contains("will not have internet access")));
    }

    #[test]
    fn wpa3_on_adapter_without_support_is_rejected() {
        let mut cfg = config();
        cfg.security = Security::Wpa3;
        let mut c = caps(true, true);
        c.supports_wpa3 = false;
        assert!(check_supported(&cfg, &c).is_err());
    }

    #[test]
    fn five_ghz_without_channels_is_rejected() {
        let mut cfg = config();
        cfg.band = Band::Band5Ghz;
        let mut c = caps(true, true);
        c.supported_channels_5ghz.clear();
        assert!(check_supported(&cfg, &c).is_err());
    }

    #[test]
    fn adapter_without_ap_mode_is_rejected() {
        let mut c = caps(true, true);
        c.supports_ap = false;
        assert!(check_supported(&config(), &c).is_err());
    }

    /// NetworkManager rejects `key-mgmt: 'wpa-psk sae'` outright, so the
    /// default transition setting has to be resolved to something it accepts.
    #[tokio::test]
    async fn transition_security_is_degraded_to_wpa2_with_a_warning() {
        let nm = FakeNm::new().with_adapter("wlan0", caps(true, true));
        let mut cfg = config();
        cfg.security = Security::Wpa2Wpa3Transition;

        let plan = plan_hotspot(&nm, &cfg, None).await.unwrap();
        assert_eq!(plan.effective_config.security, Security::Wpa2);
        assert!(plan.warnings.iter().any(|w| w.contains("WPA2")));
        // A degraded cipher is not worth interrupting the user for.
        assert!(!plan.needs_confirmation());
    }

    #[tokio::test]
    async fn explicit_security_choices_are_left_untouched() {
        let nm = FakeNm::new().with_adapter("wlan0", caps(true, true));
        for security in [Security::Wpa2, Security::Wpa3, Security::Open] {
            let mut cfg = config();
            cfg.security = security.clone();
            if security == Security::Open {
                cfg.password = String::new();
            }
            let plan = plan_hotspot(&nm, &cfg, None).await.unwrap();
            assert_eq!(plan.effective_config.security, security);
        }
    }

    /// Without `iw` the capabilities are guesses; planning must still succeed
    /// and say why the checks were skipped.
    #[tokio::test]
    async fn unknown_capabilities_do_not_block_planning() {
        let mut unknown = caps(true, true);
        unknown.detected = false;
        unknown.supports_ap = false;

        let nm = FakeNm::new().with_adapter("wlan0", unknown);
        let plan = plan_hotspot(&nm, &config(), None).await.unwrap();

        assert_eq!(plan.backend, HotspotBackend::NetworkManager);
        assert!(
            plan.warnings.iter().any(|warning| warning.contains("iw")),
            "a missing `iw` should be explained: {:?}",
            plan.warnings
        );
    }

    /// Everything the shared backend needs is present.
    const READY: &[Missing] = &[];

    #[test]
    fn a_free_adapter_uses_networkmanager() {
        // Nothing to preserve, so the simpler path that needs no root wins.
        let (backend, channel) = choose_backend(&caps(true, true), None, READY);
        assert_eq!(backend, HotspotBackend::NetworkManager);
        assert_eq!(channel, None);
    }

    /// The whole point: share the connection instead of replacing it.
    #[test]
    fn a_busy_adapter_shares_the_connection_on_the_station_channel() {
        let (backend, channel) = choose_backend(&caps(true, true), Some(&sta_link()), READY);
        assert_eq!(backend, HotspotBackend::SharedVirtualAp);
        // 5560 MHz is channel 112; both interfaces must share it.
        assert_eq!(channel, Some(112));
    }

    #[test]
    fn a_dual_channel_radio_needs_no_channel_pin() {
        let (backend, channel) = choose_backend(&caps(true, false), Some(&sta_link()), READY);
        assert_eq!(backend, HotspotBackend::SharedVirtualAp);
        assert_eq!(channel, None);
    }

    #[test]
    fn a_radio_without_concurrency_cannot_share() {
        let (backend, _) = choose_backend(&caps(false, false), Some(&sta_link()), READY);
        assert_eq!(backend, HotspotBackend::NetworkManager);
    }

    #[test]
    fn any_missing_requirement_falls_back_to_networkmanager() {
        for missing in [Missing::Root, Missing::Hostapd, Missing::Dnsmasq] {
            let (backend, _) = choose_backend(
                &caps(true, true),
                Some(&sta_link()),
                std::slice::from_ref(&missing),
            );
            assert_eq!(
                backend,
                HotspotBackend::NetworkManager,
                "{:?} should rule out sharing",
                missing
            );
        }
    }

    #[test]
    fn an_unknown_station_channel_rules_out_sharing() {
        // Without knowing the channel there is no way to match it, and a
        // mismatch would break both links.
        let mut sta = sta_link();
        sta.frequency = 1;
        let (backend, _) = choose_backend(&caps(true, true), Some(&sta), READY);
        assert_eq!(backend, HotspotBackend::NetworkManager);
    }

    #[test]
    fn sharing_leaves_the_station_connection_alone() {
        assert_eq!(
            resolve_impact(HotspotBackend::SharedVirtualAp, Some(&sta_link())),
            StaImpact::None
        );
    }

    #[test]
    fn sharing_explains_that_the_connection_is_kept() {
        let notes = backend_notes(
            HotspotBackend::SharedVirtualAp,
            &caps(true, true),
            Some(&sta_link()),
            READY,
            Some(112),
            None,
        );
        assert!(notes.iter().any(|n| n.contains("stays up")));
        assert!(notes.iter().any(|n| n.contains("channel 112")));
    }

    /// The reported machine sits on channel 100, a DFS channel, with no
    /// country configured — hostapd would refuse to start and the user needs
    /// to know why before trying.
    #[test]
    fn a_dfs_channel_without_a_country_is_called_out() {
        let notes = backend_notes(
            HotspotBackend::SharedVirtualAp,
            &caps(true, true),
            Some(&sta_link()),
            READY,
            Some(100),
            None,
        );
        assert!(notes.iter().any(|n| n.contains("radar")));
        assert!(notes.iter().any(|n| n.contains("refuse to start")));
    }

    #[test]
    fn a_dfs_channel_with_a_country_only_warns_about_the_delay() {
        let notes = backend_notes(
            HotspotBackend::SharedVirtualAp,
            &caps(true, true),
            Some(&sta_link()),
            READY,
            Some(100),
            Some("FR"),
        );
        assert!(notes.iter().any(|n| n.contains("radar")));
        assert!(!notes.iter().any(|n| n.contains("refuse to start")));
    }

    #[test]
    fn a_normal_channel_says_nothing_about_radar() {
        let notes = backend_notes(
            HotspotBackend::SharedVirtualAp,
            &caps(true, true),
            Some(&sta_link()),
            READY,
            Some(36),
            None,
        );
        assert!(!notes.iter().any(|n| n.contains("radar")));
    }

    /// The blocker found on the reporting machine: no regulatory database, so
    /// the world domain applies and 5 GHz is receive-only.
    #[test]
    fn a_missing_regulatory_database_rules_out_sharing() {
        let (backend, _) = choose_backend(
            &caps(true, true),
            Some(&sta_link()),
            &[Missing::RegulatoryDatabase],
        );
        assert_eq!(backend, HotspotBackend::NetworkManager);
    }

    #[test]
    fn a_missing_regulatory_database_names_the_package() {
        let notes = backend_notes(
            HotspotBackend::NetworkManager,
            &caps(true, true),
            Some(&sta_link()),
            &[Missing::RegulatoryDatabase],
            None,
            None,
        );
        assert!(notes.iter().any(|n| n.contains("wireless-regdb")));
    }

    #[test]
    fn a_blocked_share_says_what_is_missing() {
        let notes = backend_notes(
            HotspotBackend::NetworkManager,
            &caps(true, true),
            Some(&sta_link()),
            &[Missing::Dnsmasq],
            None,
            None,
        );
        assert!(notes.iter().any(|n| n.contains("dnsmasq")));
    }

    #[test]
    fn no_backend_note_when_sharing_was_never_possible() {
        // The radio cannot do it at all, so listing missing tools would only
        // be noise.
        let notes = backend_notes(
            HotspotBackend::NetworkManager,
            &caps(false, false),
            Some(&sta_link()),
            &[Missing::Dnsmasq],
            None,
            None,
        );
        assert!(notes.is_empty());
    }

    #[test]
    fn no_regdomain_change_when_already_in_that_country() {
        assert_eq!(resolve_regdomain("FR", Some("FR".into())), None);
        // Case and padding from user input must not force a pointless change.
        assert_eq!(resolve_regdomain(" fr ", Some("FR".into())), None);
    }

    #[test]
    fn regdomain_change_is_reported_with_both_ends() {
        assert_eq!(
            resolve_regdomain("US", Some("FR".into())),
            Some(RegDomainChange {
                from: Some("FR".into()),
                to: "US".into()
            })
        );
    }

    #[test]
    fn regdomain_change_from_unknown_current_domain() {
        assert_eq!(
            resolve_regdomain("de", None),
            Some(RegDomainChange {
                from: None,
                to: "DE".into()
            })
        );
    }

    #[tokio::test]
    async fn no_country_means_no_regdomain_change() {
        let nm = FakeNm::new().with_adapter("wlan0", caps(true, true));
        let plan = plan_hotspot(&nm, &config(), None).await.unwrap();
        assert!(plan.regdomain_change.is_none());
    }

    #[tokio::test]
    async fn invalid_country_is_rejected_before_probing() {
        let mut cfg = config();
        cfg.country_code = Some("NOPE".into());
        let nm = FakeNm::new().with_adapter("wlan0", caps(true, true));
        assert!(plan_hotspot(&nm, &cfg, None).await.is_err());
    }

    #[tokio::test]
    async fn prefers_a_spare_adapter_over_the_one_carrying_the_uplink() {
        let nm = FakeNm::new()
            .with_adapter("wlan0", caps(true, true))
            .with_adapter("wlan1", caps(true, true))
            .joined_to("wlan0", 5560);

        let plan = plan_hotspot(&nm, &config(), None).await.unwrap();
        assert_eq!(plan.ap_interface, "wlan1");
        assert_eq!(plan.impact, StaImpact::None);
        assert!(!plan.needs_confirmation());
    }

    #[tokio::test]
    async fn a_spare_adapter_wins_even_when_less_capable() {
        // Concurrency capability no longer buys anything, so staying off the
        // uplink is the only thing that matters.
        let nm = FakeNm::new()
            .with_adapter("wlan0", caps(true, false))
            .with_adapter("wlan1", caps(false, false))
            .joined_to("wlan0", 5560);

        let plan = plan_hotspot(&nm, &config(), None).await.unwrap();
        assert_eq!(plan.ap_interface, "wlan1");
        assert!(!plan.needs_confirmation());
    }

    #[tokio::test]
    async fn skips_adapters_that_cannot_do_ap_mode() {
        let mut no_ap = caps(true, true);
        no_ap.supports_ap = false;

        let nm = FakeNm::new()
            .with_adapter("wlan0", no_ap)
            .with_adapter("wlan1", caps(true, true));

        let plan = plan_hotspot(&nm, &config(), None).await.unwrap();
        assert_eq!(plan.ap_interface, "wlan1");
    }

    /// The reported setup: one radio, already joined to a network, no spare
    /// adapter. Confirmed against real hardware — this always costs the
    /// connection, and the user must be asked first.
    #[tokio::test]
    async fn single_adapter_laptop_must_ask_before_taking_the_wifi() {
        let nm = FakeNm::new()
            .with_adapter("wlp4s0", caps(true, true))
            .joined_to("wlp4s0", 5500);

        let plan = plan_hotspot(&nm, &config(), None).await.unwrap();
        assert_eq!(plan.ap_interface, "wlp4s0");
        assert!(plan.needs_confirmation());
        assert!(plan.warnings.iter().any(|w| w.contains("disconnect")));
    }

    #[tokio::test]
    async fn explicit_interface_request_is_honoured() {
        let nm = FakeNm::new()
            .with_adapter("wlan0", caps(true, true))
            .with_adapter("wlan1", caps(true, true))
            .joined_to("wlan0", 5560);

        // Asking for the uplink adapter by name is allowed, but still warned about.
        let plan = plan_hotspot(&nm, &config(), Some("wlan0")).await.unwrap();
        assert_eq!(plan.ap_interface, "wlan0");
        assert!(plan.needs_confirmation());
    }

    /// A leftover AP interface from a crashed run must not be picked as the
    /// adapter to host the next hotspot on.
    #[tokio::test]
    async fn the_ap_interface_is_never_chosen_as_the_adapter() {
        let nm = FakeNm::new()
            .with_adapter("wlp4s0", caps(true, true))
            .with_adapter(nimbus_network::virtual_ap::AP_INTERFACE, caps(true, true))
            .joined_to("wlp4s0", 5560);

        let plan = plan_hotspot(&nm, &config(), None).await.unwrap();
        assert_eq!(plan.ap_interface, "wlp4s0");
    }

    #[tokio::test]
    async fn only_a_leftover_ap_interface_is_no_adapter_at_all() {
        let nm =
            FakeNm::new().with_adapter(nimbus_network::virtual_ap::AP_INTERFACE, caps(true, true));
        assert!(plan_hotspot(&nm, &config(), None).await.is_err());
    }

    #[tokio::test]
    async fn no_wifi_adapter_is_an_error() {
        let nm = FakeNm::new();
        assert!(plan_hotspot(&nm, &config(), None).await.is_err());
    }

    #[tokio::test]
    async fn upstream_is_resolved_before_any_change() {
        let nm = FakeNm::new()
            .with_adapter("wlan0", caps(true, true))
            .upstream("enp5s0");

        let plan = plan_hotspot(&nm, &config(), None).await.unwrap();
        assert_eq!(plan.upstream_interface.as_deref(), Some("enp5s0"));
        // An ethernet uplink is untouched by the AP.
        assert!(!plan
            .warnings
            .iter()
            .any(|w| w.contains("disconnect") || w.contains("internet access")));
    }

    #[tokio::test]
    async fn invalid_config_is_rejected_before_probing() {
        let mut cfg = config();
        cfg.password = "short".into();
        let nm = FakeNm::new().with_adapter("wlan0", caps(true, true));
        assert!(plan_hotspot(&nm, &cfg, None).await.is_err());
    }
}
