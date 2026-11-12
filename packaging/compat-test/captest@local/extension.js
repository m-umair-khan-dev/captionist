// Test helper: inspects the Captionist extension from inside GNOME Shell and
// prints "CAPTEST <check> <result>" lines to the shell's log.
import GLib from 'gi://GLib';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Config from 'resource:///org/gnome/shell/misc/config.js';

const UUID = 'captionist@m-umair-khan-dev.github.io';
const out = (...a) => console.log(`CAPTEST ${a.join(' ')}`);
const later = (s, f) => GLib.timeout_add(GLib.PRIORITY_DEFAULT, s * 1000, () => {
    try { f(); } catch (e) { out('FAIL exception', e, e.stack); }
    return GLib.SOURCE_REMOVE;
});

export default class CapTest extends Extension {
    enable() {
        later(15, () => this._step1());
    }

    disable() {}

    _step1() {
        Main.overview.hide();
        out('shell-version', Config.PACKAGE_VERSION);
        const ext = Main.extensionManager.lookup(UUID);
        // state 1 = enabled/active (named ACTIVE on 50+, ENABLED before)
        out('extension-state', ext?.state, ext?.error ? `error=${ext.error}` : 'no-error');
        const c = ext?.stateObj;
        out('topbar-icon', Main.panel.statusArea['captionist'] ? 'present' : 'MISSING');
        out('quick-settings-toggle', c?._quick?.toggle ? 'present' : 'MISSING');
        const ov = c?._overlay;
        out('caption-overlay', ov?._box ? 'present' : 'MISSING');
        // Cursor: per-actor API on GNOME 48+, global cursor before.
        const path = typeof ov._box.set_cursor_type === 'function' ? 'actor (GNOME 48+)' : 'global (GNOME 46/47)';
        ov._setCursor('move'); ov._setCursor('ew-resize'); ov._setCursor('nw-resize'); ov._setCursor('default');
        out('cursor', 'ok via', path);
        // Show a caption and drag it (via settings) onto the dock.
        ov.setEnabled(true);
        ov.partial(1, 'Hello from the compatibility test');
        const s = c.settings;
        s.set_string('position', 'floating');
        s.set_int('floating-x', 200);
        s.set_int('floating-y', 5000);
        later(3, () => this._step2(ov));
    }

    _step2(ov) {
        const docks = Main.layoutManager.uiGroup.get_children().filter(a => a.name === 'dashtodockContainer');
        out('docks-found', docks.length);
        const [bx, by] = ov._box.get_transformed_position();
        const [bw, bh] = ov._box.get_transformed_size();
        out('caption-box', `x=${bx} y=${by} w=${bw} h=${bh} visible=${ov._box.visible}`);
        for (const d of docks) {
            const [dx, dy] = d.get_transformed_position();
            const [dw, dh] = d.get_transformed_size();
            const overlap = bx < dx + dw && bx + bw > dx && by < dy + dh && by + bh > dy;
            out('dock', `x=${dx} y=${dy} w=${dw} h=${dh} visible=${d.visible}`, overlap ? 'OVERLAPS-BOX' : 'clear-of-box');
        }
        const mon = Main.layoutManager.primaryMonitor;
        out('monitor', `${mon.width}x${mon.height}`);
        out('done');
    }
}
