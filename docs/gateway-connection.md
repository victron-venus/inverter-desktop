# Gateway connection

Remote Gateway supports native HTTPS inverter-gateway endpoints and public
endpoints protected by Cloudflare Access.

For a native endpoint, enter its HTTPS URL and the gateway's API bearer token,
then leave both Cloudflare Access fields blank. Native gateways usually require
the bearer token. The app also preserves support for gateway configurations that
do not require it; authentication is enforced by the gateway.

For an endpoint protected by Cloudflare Access, supply both its Access Client ID
and Access Client Secret. Continue to supply the gateway bearer token if that
gateway requires it. A partially filled Access pair is rejected in setup,
configuration testing and connection selection.

The Access Client ID and Secret belong in the connecting client's configuration;
Cloudflare Access checks them before forwarding traffic. The IGW server uses its
own API/read bearer tokens and does not require the Access service-token pair.
The separate Cloudflare Tunnel token belongs to the `cloudflared` connector.

The connection uses HTTPS with system certificate verification and does not
follow redirects. The URL's hostname must resolve to the gateway and match its
trusted certificate. Native HTTPS does not require a public hostname or a
Cloudflare account. Settings and first-run setup use the same validation policy.

When MQTT and IGW are both configured, the existing connection policy still
prefers reachable MQTT and uses IGW for failover.
