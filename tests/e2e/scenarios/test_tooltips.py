# SPDX-License-Identifier: MIT
"""One hover delay owns normal, browse, and native-popover text tooltips."""

import time

from test_custom_actions import _open_new_action


def visible_tooltip(strata, text):
    for tooltip in strata.window.find_all(role="tool tip"):
        label = tooltip.find(role="label", name=text)
        if label is not None and label.has_state("showing"):
            return label
    return None


def test_browse_tooltips_wait_and_do_not_intercept_clicks(strata):
    settings = strata.window.find(role="button", name="Settings")
    appearance = strata.window.find(role="button", name="Appearance")
    strata.pointer.move_to(*settings.screen_bounds().center)
    strata.pointer.connection.button(2, True)
    try:
        strata.pointer.move_to(*appearance.screen_bounds().center)
        time.sleep(0.6)
        assert not visible_tooltip(strata, "Appearance"), "held buttons suppress pointer tooltips"
    finally:
        strata.pointer.connection.button(2, False)
    strata.pointer.move_to(*settings.screen_bounds().center)
    strata.wait(lambda: visible_tooltip(strata, "Settings"), "first tooltip")
    strata.pointer.move_to(*appearance.screen_bounds().center)
    time.sleep(0.2)
    assert not visible_tooltip(strata, "Settings")
    assert not visible_tooltip(strata, "Appearance"), "browse mode must not shorten the delay"
    strata.wait(lambda: visible_tooltip(strata, "Appearance"), "tooltip after stationary hover")
    strata.keyboard.press("ctrl+k")
    strata.editable_field()
    assert not visible_tooltip(strata, "Appearance"), "handled shortcuts also dismiss tooltips"
    strata.keyboard.press("Escape")
    strata.wait(lambda: strata.window.find(role="text", states={"editable"}) is None, "search palette closes")
    strata.park_pointer()
    appearance = strata.window.find(role="button", name="Appearance")
    settings = strata.window.find(role="button", name="Settings")
    strata.pointer.move_to(*appearance.screen_bounds().center)
    strata.wait(lambda: visible_tooltip(strata, "Appearance"), "tooltip after hovering again")
    strata.pointer.click(settings)
    strata.wait(lambda: strata.window.find(role="button", name="Actions"), "Settings remains clickable")
    time.sleep(0.6)
    assert not visible_tooltip(strata, "Appearance")
    assert not visible_tooltip(strata, "Settings"), "a click cancels the pending hover"


def test_native_popover_tooltips_wait_and_cancel_when_the_source_closes(strata):
    dialog = _open_new_action(strata)
    strata.pointer.click(dialog.find(name="More icons"))
    first = strata.wait(lambda: dialog.find(name="code-xml icon"), "popup icon")
    second = dialog.find(name="file-text icon")
    strata.pointer.move_to(*first.screen_bounds().center)
    strata.wait(lambda: visible_tooltip(strata, "code-xml"), "native-popover tooltip")
    strata.pointer.move_to(*second.screen_bounds().center)
    time.sleep(0.2)
    assert not visible_tooltip(strata, "code-xml")
    assert not visible_tooltip(strata, "file-text")
    strata.wait(lambda: visible_tooltip(strata, "file-text"), "next popup tooltip")
    strata.pointer.click(second)
    strata.wait(lambda: dialog.find(name="file-text icon") is None, "icon choice dismisses the popup")
    time.sleep(0.6)
    assert not visible_tooltip(strata, "file-text"), "closing the source cancels its tooltip"
