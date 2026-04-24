# Changelog

## [2.7.9] - 2026-04-24
### Fixed
- Repair completion status "OK" was overriding header sys_status, causing "OK | feed hidden" to display in red instead of falling back to base status (LIVE/CONNECTING/DOWN)
- Removed dead color branch in `header_attr()` that handled "OK" — unreachable after above fix

## [2.7.8] - 2026-04-24
### Fixed
- share/index.html out of sync with full feature set (buzz, dark, message)
- Debian nginx config missing /api/ proxy block
- broadcast-api not enabled/started by setup
- sudoers files shipped with uid 1000 instead of root ownership
- hls-livecam-dark sudoers missing www-data entry
- DEBIAN/control version was 2.2.0
- DEBIAN/control missing python3-pil and python3-flask dependencies
- camstack version string was 2.6.3
- camstack-smart sudoers used %wheel instead of %sudo
- hls-livecam-dark sudoers hardcoded username
- Arch setup: wrong service file copy paths, no sha256 verification, double MediaMTX download, missing video/sudo group membership, missing sudoers for [h] and [s]
- postinst missing /var/lib/hls-livecam/ directory creation
- hls-livecam-dark missing mkdir -p guard
- prerm missing broadcast-api stop and sudoers cleanup
- PKGBUILD: v4l2-utils → v4l-utils, missing python-pillow, baked index.html removed, missing .install file
- dark.png served without no-cache headers
- block_art.py freeze frame re-landed and working

## [2.7.5] - 2026-04-10
### Notes
- Last AUR-published version prior to 2.7.8 audit
