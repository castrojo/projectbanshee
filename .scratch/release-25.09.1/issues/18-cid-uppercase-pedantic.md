# 18: appstreamcli pedantic hint: component ID has uppercase letters

**What to build:** `appstreamcli validate --pedantic` reports `P: cid-contains-uppercase-letter io.github.castrojo.Banshee`. The default validation passes. Renaming the app ID would break installs, App Memory paths and the MPRIS/D-Bus name.

**Blocked by:** None

**Status:** wontfix

**GitHub:** https://github.com/castrojo/projectbanshee/issues/18

## Comments

