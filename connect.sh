#!/bin/zsh
# GEWIS Remote Desktop — connect

BREW=$(brew --prefix 2>/dev/null)
XFREERDP="$HOME/opt/freerdp-krb5/bin/xfreerdp"
KINIT="${BREW}/opt/krb5/bin/kinit"
CCACHE="FILE:/tmp/krb5cc_gewis_rdp"

# ── Sanity check ─────────────────────────────────────────────────────────────
if [[ ! -x "$XFREERDP" ]]; then
    echo "FreeRDP not found. Run ~/gewis-rdp/setup.sh first."
    exit 1
fi

# ── Credentials ──────────────────────────────────────────────────────────────
echo -n "Member number (e.g. m11494): "
read -r member

if [[ -z "$member" ]]; then
    echo "No member number entered."
    exit 1
fi

echo -n "Password: "
read -rs password
echo

# ── Kerberos ticket ───────────────────────────────────────────────────────────
echo "Getting Kerberos ticket..."
if ! printf '%s\n' "$password" | \
        "$KINIT" -c "$CCACHE" "${member}@GEWISWG.GEWIS.NL" 2>&1; then
    echo ""
    echo "Failed to get a Kerberos ticket."
    echo "Check your member number and password, then try again."
    exit 1
fi

# ── Launch ────────────────────────────────────────────────────────────────────
echo "Connecting — opening XQuartz..."
open -a XQuartz
sleep 3

DISPLAY=:0 KRB5CCNAME="$CCACHE" \
    "$XFREERDP" \
    /v:gewisvdesktop.gewis.nl \
    /u:"$member" \
    /d:GEWISWG.GEWIS.NL \
    "/p:${password}" \
    /sec:nla \
    "/gateway:g:gewisvdesktop.gewis.nl,u:${member},d:GEWISWG.GEWIS.NL,p:${password},type:http" \
    /cert:ignore \
    +credentials-delegation
