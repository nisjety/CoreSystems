from app.control_plane_subscriber import CONTROL_PLANE_SUBJECTS


def test_subscriber_uses_the_subjects_control_plane_actually_publishes() -> None:
    assert CONTROL_PLANE_SUBJECTS == {
        "provider_linked": "aqencia.controlplane.user.provider_linked",
        "plan_changed": "aqencia.controlplane.org.plan_changed",
        "quota_exceeded": "aqencia.controlplane.billing.quota_exceeded",
    }
