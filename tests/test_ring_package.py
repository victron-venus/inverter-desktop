"""Ring camera metadata, native target staging and bounded snapshot authority."""

import unittest

from camera_package_helpers import validate_camera_staging


class RingPackageTests(unittest.TestCase):
    """Snapshot authority is declared separately from the MQTT credentials."""

    def test_native_packages_bind_explicit_snapshot_credentials_and_cadence(self):
        """The host applies Ring's 20-second cadence and exact configured scope."""
        manifest = validate_camera_staging(self, "ring")
        self.assertEqual(set(manifest["permissions"]), {
            "dashboard_contributions", "plugin_configuration", "network_mqtt",
            "desktop_notifications", "http_video",
        })
        self.assertNotIn("live_view", manifest)
        self.assertEqual(manifest["http_video"], {
            "base_url_setting": "snapshot_base_url",
            "bearer_token_setting": "snapshot_bearer_token",
            "cooldown_seconds": 20,
            "allow_query": True,
        })
        schema = manifest["config_schema"]
        self.assertEqual(schema["required"], ["mqtt_host"])
        fields = schema["properties"]
        self.assertEqual(fields["mqtt_topics"]["default"],
                         "ring/+/camera/+/motion/state;ring/+/camera/+/ding/state")
        private = {key for key, field in fields.items() if field.get("writeOnly")}
        self.assertEqual(private, {
            "mqtt_username", "mqtt_password", "snapshot_url_template", "snapshot_bearer_token",
        })
        self.assertEqual(fields["snapshot_media_kind"]["enum"], ["jpeg", "png", "webp", "video"])
        self.assertEqual(fields["snapshot_media_kind"]["default"], "jpeg")
        self.assertNotIn("default", fields["snapshot_url_template"])
        self.assertNotIn("default", fields["snapshot_bearer_token"])
        self.assertNotIn("ha_token", fields)


if __name__ == "__main__":
    unittest.main()
