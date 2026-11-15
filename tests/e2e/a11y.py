"""Small AT-SPI helpers: find widgets, run their actions, read their state."""
import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def find(root, role, name, showing=True):
    out = []

    def w(n):
        try:
            if (n.get_role_name() == role and n.get_name() == name
                    and (not showing or n.get_state_set().contains(Atspi.StateType.SHOWING))):
                out.append(n)
            for i in range(n.get_child_count()):
                w(n.get_child_at_index(i))
        except Exception:
            pass
    w(root)
    return out


def actions(n):
    a = n.get_action_iface()
    return [a.get_action_name(i) for i in range(a.get_n_actions())] if a else []


def do(n, act=None):
    names = actions(n)
    n.get_action_iface().do_action(names.index(act) if act else 0)


def checked(n):
    return n.get_state_set().contains(Atspi.StateType.CHECKED)


def text(n):
    try:
        return Atspi.Text.get_text(n, 0, Atspi.Text.get_character_count(n)) if n.get_text_iface() else None
    except Exception:
        return None


def value(n):
    try:
        return Atspi.Value.get_current_value(n) if n.get_value_iface() else None
    except Exception:
        return None


def focused(root):
    out = []

    def w(n):
        try:
            st = n.get_state_set()
            if st.contains(Atspi.StateType.FOCUSED):
                out.append(n)
            if not st.contains(Atspi.StateType.SHOWING) and n.get_role_name() not in (
                    "application", "frame", "window", "dialog"):
                return
            for i in range(n.get_child_count()):
                w(n.get_child_at_index(i))
        except Exception:
            pass
    w(root)
    return out
