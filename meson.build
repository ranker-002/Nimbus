project('nimbus-hotspot', 'rust',
  version: '0.1.0',
  license: 'MIT',
  meson_version: '>= 0.62',
  default_options: [
    'warning_level=2',
  ],
)

dependency('glib-2.0', version: '>= 2.66')
dependency('gio-2.0', version: '>= 2.66')
dependency('gtk4', version: '>= 4.12')
dependency('libadwaita-1', version: '>= 1.4')

cargo = find_program('cargo', required: true)
source_dir = meson.current_source_dir()
bindir = join_paths(get_option('prefix'), get_option('bindir'))

# One cargo invocation builds all three binaries; the copies chain through the
# same shell so meson gets every declared output. (`@SOURCE_DIR@` is not
# expanded in custom_target commands, hence the direct interpolation. The
# manifest path is quoted so directories with spaces still work.)
cargo_build = cargo.full_path() + ' build --manifest-path "' + source_dir + '/Cargo.toml" --target-dir @OUTDIR@/target --release'

custom_target('cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: ['nimbus-hotspot', 'nimbus', 'nimbus-dbus-service'],
  console: true,
  install: true,
  install_dir: [
    get_option('bindir'),
    get_option('bindir'),
    get_option('bindir'),
  ],
  command: [
    'sh', '-c',
    cargo_build +
    ' && cp @OUTDIR@/target/release/nimbus-hotspot @OUTPUT0@' +
    ' && cp @OUTDIR@/target/release/nimbus @OUTPUT1@' +
    ' && cp @OUTDIR@/target/release/nimbus-dbus-service @OUTPUT2@',
  ],
)

install_data(
  'data/com.nimbus.Hotspot.desktop',
  install_dir: join_paths(get_option('datadir'), 'applications'),
)

install_data(
  'data/com.nimbus.Hotspot.metainfo.xml',
  install_dir: join_paths(get_option('datadir'), 'metainfo'),
)

install_data(
  'data/com.nimbus.Hotspot.svg',
  install_dir: join_paths(get_option('datadir'), 'icons', 'hicolor', 'scalable', 'apps'),
)

# D-Bus service and its bus policy: the service runs as root because hostapd,
# dnsmasq, nftables and `iw reg set` all need CAP_NET_ADMIN.
install_data(
  'data/com.nimbus.Hotspot.conf',
  install_dir: join_paths(get_option('datadir'), 'dbus-1', 'system.d'),
)

install_data(
  'data/com.nimbus.Hotspot.policy',
  install_dir: join_paths(get_option('datadir'), 'polkit-1', 'actions'),
)

conf = configuration_data()
conf.set('BINDIR', bindir)

systemd_service = configure_file(
  input: 'data/com.nimbus.Hotspot.service.in',
  output: 'com.nimbus.Hotspot.service',
  configuration: conf,
)

install_data(
  systemd_service,
  install_dir: join_paths(get_option('prefix'), 'lib', 'systemd', 'system'),
)
