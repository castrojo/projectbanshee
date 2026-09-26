project('banshee',
  version: '0.1.0',
  meson_version: '>= 0.60.0',
  default_options: ['warning_level=2']
)

python = import('python').find_installation('python3')

# Install python modules
python.install_sources(
  'src/banshee/__init__.py',
  'src/banshee/main.py',
  'src/banshee/models.py',
  'src/banshee/player.py',
  'src/banshee/mpris.py',
  subdir: 'banshee'
)

python.install_sources(
  'src/banshee/sources/__init__.py',
  'src/banshee/sources/base.py',
  'src/banshee/sources/ytm.py',
  subdir: 'banshee/sources'
)

python.install_sources(
  'src/banshee/ui/__init__.py',
  'src/banshee/ui/window.py',
  'src/banshee/ui/popovers.py',
  subdir: 'banshee/ui'
)

# Install executable script
install_data('bin/banshee', install_dir: get_option('bindir'), install_mode: 'rwxr-xr-x')

# Install desktop entry, metainfo, and icons
install_data('data/org.projectbluefin.Banshee.desktop', install_dir: get_option('datadir') / 'applications')
install_data('data/org.projectbluefin.Banshee.metainfo.xml', install_dir: get_option('datadir') / 'metainfo')
install_data('data/icons/hicolor/scalable/apps/org.projectbluefin.Banshee.svg',
  install_dir: get_option('datadir') / 'icons/hicolor/scalable/apps'
)
install_data('data/icons/hicolor/128x128/apps/org.projectbluefin.Banshee.png',
  install_dir: get_option('datadir') / 'icons/hicolor/128x128/apps'
)

# Run tests
test('unit-tests', python, args: ['-m', 'unittest', 'discover', '-s', meson.project_source_root() / 'tests', '-p', 'test_*.py'])
