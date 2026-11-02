from app.control_plane_subscriber import CONTROL_PLANE_SUBJECTS


def test_subscriber_uses_the_subjects_control_plane_actually_publishes() -> None:
    assert CONTROL_PLANE_SUBJECTS == {
        "provider_linked": "controlplane.user.provider_linked",
        "plan_changed": "controlplane.org.plan_changed",
        "quota_exceeded": "controlplane.billing.quota_exceeded",
    }
