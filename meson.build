project('nimbus-hotspot', 'rust',
  version: '0.1.0',
  license: 'GPL-3.0-or-later',
  meson_version: '>= 0.62',
  default_options: [
    'warning_level=2',
  ],
)

i18n = import('i18n')
gnome = import('gnome')

base_id = 'com.nimbus.Hotspot'

dependency('glib-2.0', version: '>= 2.66')
dependency('gio-2.0', version: '>= 2.66')
dependency('gtk4', version: '>= 4.12')
dependency('libadwaita-1', version: '>= 1.4')

cargo = find_program('cargo', required: true)
source_dir = meson.current_source_dir()

custom_target('cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: 'nimbus-hotspot',
  console: true,
  install: true,
  install_dir: get_option('bindir'),
  command: [
    cargo, 'build',
    '--manifest-path', '@SOURCE_DIR@/Cargo.toml',
    '--target-dir', '@OUTDIR@/target',
    '--release',
    '&&',
    'cp', '@OUTDIR@/target/release/nimbus-hotspot', '@OUTPUT@',
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
  'data/com.nimbus.Hotspot.gschema.xml',
  install_dir: join_paths(get_option('datadir'), 'glib-2.0', 'schemas'),
)

subdir('data')
