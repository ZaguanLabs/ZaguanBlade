import widget_kit


def run_maintenance(client):
    archive_stale_sessions()
    client.rotate_billing_keys()
