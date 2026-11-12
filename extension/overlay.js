// Caption window: a small, fixed-size floating box. It starts centred at the
// bottom (above the dock) or top of the screen, and can be dragged anywhere;
// the position is remembered. Choosing Top/Bottom again snaps it back.

import GLib from 'gi://GLib';
import St from 'gi://St';
import Clutter from 'gi://Clutter';
import Pango from 'gi://Pango';
import Meta from 'gi://Meta';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {CaptionControls} from './controls.js';

const MAX_SEGMENTS = 40;
const PAD_X = 20;
const PAD_Y = 10;
/** Gap between the caption window and the screen edge / dock. */
const MARGIN = 24;
/** Width of the edge zone that resizes instead of moving (px). */
const EDGE = 10;
/** Actor names of the docks the caption box keeps clear of (Ubuntu Dock and Dash to Dock). */
const DOCK_NAMES = ['dashtodockContainer'];

/** Cursor names → enum member names, newest first (names differ between GNOME versions). */
const CURSORS = {
    'default': ['DEFAULT'],
    'move': ['MOVE', 'MOVE_OR_RESIZE_WINDOW'],
    'nw-resize': ['NW_RESIZE'],
    'ne-resize': ['NE_RESIZE'],
    'sw-resize': ['SW_RESIZE'],
    'se-resize': ['SE_RESIZE'],
    'ew-resize': ['EW_RESIZE', 'EAST_RESIZE'],
    'ns-resize': ['NS_RESIZE', 'SOUTH_RESIZE'],
};

function cursorValue(cursorEnum, name) {
    const key = CURSORS[name].find(k => k in cursorEnum);
    return cursorEnum[key ?? 'DEFAULT'];
}
const MIN_WIDTH = 280;
const STYLE_KEYS = [
    'font-size', 'lines', 'width-percent', 'text-color',
    'bg-color', 'bg-opacity', 'dim-drafts', 'floating-x', 'floating-y',
];

function rgba(hex, opacityPct) {
    const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex) ?? [0, '00', '00', '00'];
    const [r, g, b] = [m[1], m[2], m[3]].map(h => parseInt(h, 16));
    return `rgba(${r}, ${g}, ${b}, ${Math.max(0, Math.min(100, opacityPct)) / 100})`;
}

export class CaptionOverlay {
    /** @param {object} callbacks {onClose, onSettings} */
    constructor(settings, callbacks = {}) {
        this._settings = settings;
        this._segments = new Map(); // id -> {text, final}
        this._message = '';
        this._messageSticky = false;
        this._enabled = false;
        this._hideId = 0;
        this._drag = null;

        this._box = new St.Widget({
            style_class: 'captionist-overlay',
            layout_manager: new Clutter.BinLayout(),
            reactive: true,
            track_hover: true,
            clip_to_allocation: true,
            visible: false,
        });
        this._label = new St.Label({
            style_class: 'captionist-text',
            x_expand: true,
            y_expand: true,
            y_align: Clutter.ActorAlign.END,
        });
        const ct = this._label.clutter_text;
        ct.line_wrap = true;
        ct.line_wrap_mode = Pango.WrapMode.WORD_CHAR;
        ct.ellipsize = Pango.EllipsizeMode.NONE;
        this._box.add_child(this._label);
        this._controls = new CaptionControls(this._box, settings, {
            onClose: () => callbacks.onClose?.(),
            onSettings: () => callbacks.onSettings?.(),
            isResizing: () => !!this._resize,
        });
        // No trackFullscreen: GNOME would then set the box's visibility itself
        // (showing an empty box at login) and hide captions over fullscreen
        // videos. Visibility is controlled only by _updateVisibility().
        Main.layoutManager.addChrome(this._box);

        this._settingIds = STYLE_KEYS.map(k =>
            settings.connect(`changed::${k}`, () => this._relayout()));
        // Picking Top/Bottom snaps the window back to that edge.
        this._settingIds.push(settings.connect('changed::position', () => {
            this._settings.set_int('floating-x', -1);
            this._settings.set_int('floating-y', -1);
            this._relayout();
        }));
        this._monitorsId = Main.layoutManager.connect('monitors-changed', () => this._relayout());
        this._workareasId = global.display.connect('workareas-changed', () => this._relayout());
        /** dock actor → its signal ids (see _dockRects). */
        this._watchedDocks = new Map();
        this._relayoutId = 0;
        // A dock may be added after us (extension load order): check again then.
        this._uiGroupId = Main.layoutManager.uiGroup.connect('child-added', (_g, child) => {
            if (DOCK_NAMES.includes(child.name))
                this._queueRelayout();
        });
        this._setupDrag();
        this._relayout();
    }

    destroy() {
        this._settingIds.forEach(id => this._settings.disconnect(id));
        Main.layoutManager.disconnect(this._monitorsId);
        global.display.disconnect(this._workareasId);
        Main.layoutManager.uiGroup.disconnect(this._uiGroupId);
        for (const [dock, ids] of this._watchedDocks)
            ids.forEach(id => dock.disconnect(id));
        this._watchedDocks.clear();
        if (this._relayoutId)
            GLib.source_remove(this._relayoutId);
        if (this._hideId)
            GLib.source_remove(this._hideId);
        this._grab?.dismiss();
        this._controls.destroy();
        Main.layoutManager.removeChrome(this._box);
        this._box.destroy();
    }

    // ── public API ─────────────────────────────────────────────────────────

    setEnabled(on) {
        this._enabled = on;
        if (!on) {
            this._segments.clear();
            this._message = '';
        }
        this._updateVisibility();
        this._render();
    }

    /** Status line shown while there are no captions (loading, problems…). */
    setMessage(text, sticky = false) {
        this._message = text ?? '';
        this._messageSticky = sticky;
        if (this._message)
            this._activity();
        else
            this._render();
    }

    partial(id, text) {
        if (!this._enabled || !text)
            return;
        this._segments.set(id, {text, final: false});
        this._prune();
        this._activity();
    }

    final(id, text) {
        if (!this._enabled)
            return;
        if (!text) {
            this._segments.delete(id);
        } else {
            const isNew = !this._segments.has(id);
            this._segments.set(id, {text, final: true});
            if (isNew)
                this._segments = new Map([...this._segments.entries()].sort((a, b) => a[0] - b[0]));
        }
        this._prune();
        this._activity();
    }

    // ── internals ──────────────────────────────────────────────────────────

    _prune() {
        while (this._segments.size > MAX_SEGMENTS)
            this._segments.delete(this._segments.keys().next().value);
    }

    _activity() {
        if (!this._enabled)
            return;
        if (!this._messageSticky && this._segments.size > 0)
            this._message = '';
        this._updateVisibility(true);
        this._render();

        if (this._hideId)
            GLib.source_remove(this._hideId);
        this._hideId = 0;
        const after = this._settings.get_int('hide-after');
        if (after > 0) {
            this._hideId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, after, () => {
                this._hideId = 0;
                // Don't hide while the mouse is over it, its menu is open, or
                // it's being moved or resized.
                if (this._drag || this._resize || this._controls.busy)
                    return GLib.SOURCE_CONTINUE;
                this._segments.clear();
                if (!this._messageSticky)
                    this._message = '';
                this._render();
                this._updateVisibility(false);
                return GLib.SOURCE_REMOVE;
            });
        }
    }

    _updateVisibility(hasContent = this._segments.size > 0 || !!this._message) {
        this._box.visible = this._enabled && (hasContent || this._controls?.busy);
    }

    _scale() {
        return St.ThemeContext.get_for_stage(global.stage).scale_factor;
    }

    /** Real height of one text line in the caption font (device pixels). */
    _lineHeight(fontSize) {
        try {
            const layout = Pango.Layout.new(this._label.get_pango_context());
            layout.set_font_description(this._label.get_theme_node().get_font());
            layout.set_text('Ãgjy', -1);
            const [, logical] = layout.get_pixel_extents();
            if (logical.height > 0)
                return Math.ceil(logical.height);
        } catch {}
        return Math.ceil(fontSize * 1.4 * this._scale());
    }

    /**
     * Screen rectangles of visible docks (Ubuntu Dock / Dash to Dock), or an
     * empty list on a plain GNOME desktop. Also watches each dock so the box is
     * re-checked when a dock appears, moves or changes size.
     */
    _dockRects() {
        const docks = Main.layoutManager.uiGroup.get_children()
            .filter(a => DOCK_NAMES.includes(a.name));
        for (const dock of docks) {
            if (this._watchedDocks.has(dock))
                continue;
            const ids = ['notify::allocation', 'notify::visible', 'notify::mapped']
                .map(sig => dock.connect(sig, () => this._queueRelayout()));
            ids.push(dock.connect('destroy', () => this._watchedDocks.delete(dock)));
            this._watchedDocks.set(dock, ids);
        }
        return docks
            .filter(d => d.visible && d.mapped)
            .map(d => {
                const [x, y] = d.get_transformed_position();
                const [w, h] = d.get_transformed_size();
                return {x, y, w, h};
            })
            .filter(r => r.w > 0 && r.h > 0);
    }

    /** Moves the box (x, y, w × h) clear of any dock it overlaps. */
    _avoidDocks(x, y, w, h, mon, gap) {
        for (const d of this._dockRects()) {
            const overlaps = x < d.x + d.w && x + w > d.x && y < d.y + d.h && y + h > d.y;
            if (!overlaps)
                continue;
            const horizontal = d.w >= d.h;
            if (horizontal && d.y + d.h / 2 > mon.y + mon.height / 2)
                y = d.y - h - gap;              // dock at the bottom: sit above it
            else if (horizontal)
                y = d.y + d.h + gap;            // dock at the top: sit below it
            else if (d.x + d.w / 2 < mon.x + mon.width / 2)
                x = d.x + d.w + gap;            // dock on the left: sit right of it
            else
                x = d.x - w - gap;              // dock on the right: sit left of it
        }
        x = Math.min(Math.max(x, mon.x), mon.x + mon.width - w);
        y = Math.min(Math.max(y, mon.y), mon.y + mon.height - h);
        return [Math.round(x), Math.round(y)];
    }

    /** Re-lays out once on the next idle (docks animate in several steps). */
    _queueRelayout() {
        if (this._relayoutId)
            return;
        this._relayoutId = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            this._relayoutId = 0;
            if (!this._drag && !this._resize)
                this._relayout();
            return GLib.SOURCE_REMOVE;
        });
    }

    _relayout() {
        const s = this._settings;
        const mon = Main.layoutManager.primaryMonitor;
        if (!mon)
            return;
        // Work area = screen minus top bar and dock.
        const wa = Main.layoutManager.getWorkAreaForMonitor(Main.layoutManager.primaryIndex);
        const scale = this._scale();
        const fontSize = s.get_int('font-size');

        this._box.style = `background-color: ${rgba(s.get_string('bg-color'), s.get_int('bg-opacity'))};` +
            `border-radius: 14px; padding: ${PAD_Y}px ${PAD_X}px;`;
        this._label.style = `font-size: ${fontSize}px; color: ${s.get_string('text-color')};`;

        const lines = s.get_int('lines');
        if (this._resize)
            return; // the user is resizing; position/size are set live
        const width = Math.max(MIN_WIDTH * scale, Math.round(wa.width * s.get_int('width-percent') / 100));
        this._lineHeightPx = this._lineHeight(fontSize);
        const height = lines * this._lineHeightPx + 2 * PAD_Y * scale;

        let x = s.get_int('floating-x');
        let y = s.get_int('floating-y');
        if (x < 0 || y < 0) {
            x = wa.x + Math.round((wa.width - width) / 2);
            y = s.get_string('position') === 'top'
                ? wa.y + MARGIN * scale
                : wa.y + wa.height - height - MARGIN * scale;
        }
        // Keep it on screen even if the monitor layout changed.
        x = Math.min(Math.max(x, mon.x), mon.x + mon.width - width);
        y = Math.min(Math.max(y, mon.y), mon.y + mon.height - height);
        // If a dock covers the box, move the box next to it. The saved
        // position stays as is, so the box returns if the dock goes away.
        [x, y] = this._avoidDocks(x, y, width, height, mon, MARGIN * scale);

        this._box.set_position(x, y);
        this._box.set_size(width, height);
        this._textWidth = width - 2 * PAD_X * scale;
        this._updateVisibility();
        this._render();
    }

    /** Character index where the last `lines` wrapped lines of `text` begin. */
    _cutIndex(text) {
        const lines = this._settings.get_int('lines');
        let font;
        try {
            font = this._label.get_theme_node().get_font();
        } catch {
            return Math.max(0, text.length - lines * 60);
        }
        const layout = Pango.Layout.new(this._label.get_pango_context());
        layout.set_font_description(font);
        layout.set_width(Math.max(50, this._textWidth - 4) * Pango.SCALE);
        layout.set_wrap(Pango.WrapMode.WORD_CHAR);
        layout.set_text(text, -1);
        const n = layout.get_line_count();
        if (n <= lines)
            return 0;
        const byteIdx = layout.get_line_readonly(n - lines).start_index;
        const bytes = new TextEncoder().encode(text);
        return new TextDecoder().decode(bytes.slice(0, byteIdx)).length;
    }

    _render() {
        const parts = [...this._segments.values()];
        if (parts.length === 0) {
            if (this._message) {
                const esc = GLib.markup_escape_text(this._message, -1);
                this._label.clutter_text.set_markup(`<span alpha="70%"><i>${esc}</i></span>`);
            } else {
                this._label.clutter_text.set_text('');
            }
            return;
        }
        const plain = parts.map(p => p.text).join(' ');
        const cut = this._cutIndex(plain);
        const dim = this._settings.get_boolean('dim-drafts');
        let pos = 0;
        const out = [];
        for (const p of parts) {
            const start = pos;
            const end = pos + p.text.length;
            pos = end + 1;
            if (end <= cut)
                continue;
            const esc = GLib.markup_escape_text(p.text.slice(Math.max(0, cut - start)), -1);
            out.push(!p.final && dim ? `<span alpha="62%">${esc}</span>` : esc);
        }
        this._label.clutter_text.set_markup(out.join(' '));
    }

    /** Which edges of the box are under local point (x, y): {left, right, top, bottom}. */
    _edgesAt(x, y) {
        const [w, h] = this._box.get_size();
        const t = EDGE * this._scale();
        return {left: x < t, right: x > w - t, top: y < t, bottom: y > h - t};
    }

    _cursorFor(e) {
        if ((e.top && e.left) || (e.bottom && e.right))
            return e.top ? 'nw-resize' : 'se-resize';
        if ((e.top && e.right) || (e.bottom && e.left))
            return e.top ? 'ne-resize' : 'sw-resize';
        if (e.left || e.right)
            return 'ew-resize';
        if (e.top || e.bottom)
            return 'ns-resize';
        return 'move';
    }

    /**
     * Sets the pointer shape over the box. GNOME 48+ sets it per actor
     * (set_cursor_type); older GNOME (46/47) only has a global cursor.
     */
    _setCursor(name) {
        if (typeof this._box.set_cursor_type === 'function' && Clutter.CursorType)
            this._box.set_cursor_type(cursorValue(Clutter.CursorType, name));
        else
            global.display.set_cursor(cursorValue(Meta.Cursor, name));
    }

    /**
     * Move by dragging inside the box; resize by dragging any edge or corner
     * (like a normal window). Height snaps to whole lines on release.
     */
    _setupDrag() {
        const isOwn = event => {
            const target = global.stage.get_event_actor(event);
            return target === this._box || target === this._label || target === this._label.clutter_text ||
                this._controls?.isGrip(target);
        };
        this._box.connect('button-press-event', (_a, event) => {
            if (event.get_button() !== Clutter.BUTTON_PRIMARY || !isOwn(event))
                return Clutter.EVENT_PROPAGATE; // ⋮ and × buttons handle their own clicks
            const [sx, sy] = event.get_coords();
            const [, lx, ly] = this._box.transform_stage_point(sx, sy);
            let edges = this._edgesAt(lx, ly);
            if (this._controls?.isGrip(global.stage.get_event_actor(event)))
                edges = {left: false, top: false, right: true, bottom: true};
            const [bx, by] = this._box.get_position();
            const [w, h] = this._box.get_size();
            if (edges.left || edges.right || edges.top || edges.bottom)
                this._resize = {edges, sx, sy, bx, by, w, h};
            else
                this._drag = {dx: sx - bx, dy: sy - by};
            this._grab = global.stage.grab(this._box);
            this._controls?.sync();
            return Clutter.EVENT_STOP;
        });

        this._box.connect('motion-event', (_a, event) => {
            const [sx, sy] = event.get_coords();
            if (this._drag) {
                this._box.set_position(Math.round(sx - this._drag.dx), Math.round(sy - this._drag.dy));
                return Clutter.EVENT_STOP;
            }
            if (this._resize) {
                const r = this._resize;
                const scale = this._scale();
                const minW = MIN_WIDTH * scale;
                const minH = (this._lineHeightPx ?? 32) + 2 * PAD_Y * scale;
                const dx = sx - r.sx, dy = sy - r.sy;
                let {bx: x, by: y, w, h} = r;
                if (r.edges.right)
                    w = Math.max(minW, r.w + dx);
                if (r.edges.left) {
                    w = Math.max(minW, r.w - dx);
                    x = r.bx + r.w - w;
                }
                if (r.edges.bottom)
                    h = Math.max(minH, r.h + dy);
                if (r.edges.top) {
                    h = Math.max(minH, r.h - dy);
                    y = r.by + r.h - h;
                }
                this._box.set_position(Math.round(x), Math.round(y));
                this._box.set_size(Math.round(w), Math.round(h));
                return Clutter.EVENT_STOP;
            }
            // Hovering: show which action a press would start.
            if (isOwn(event)) {
                const [, lx, ly] = this._box.transform_stage_point(sx, sy);
                const onGrip = this._controls?.isGrip(global.stage.get_event_actor(event));
                this._setCursor(onGrip ? 'se-resize' : this._cursorFor(this._edgesAt(lx, ly)));
            }
            return Clutter.EVENT_PROPAGATE;
        });

        this._box.connect('leave-event', () => {
            if (!this._drag && !this._resize)
                this._setCursor('default');
            return Clutter.EVENT_PROPAGATE;
        });

        this._box.connect('button-release-event', () => {
            if (!this._drag && !this._resize)
                return Clutter.EVENT_PROPAGATE;
            this._grab?.dismiss();
            this._grab = null;
            const [x, y] = this._box.get_position();
            if (this._resize) {
                const [w, h] = this._box.get_size();
                const wa = Main.layoutManager.getWorkAreaForMonitor(Main.layoutManager.primaryIndex);
                const lines = Math.round((h - 2 * PAD_Y * this._scale()) / (this._lineHeightPx ?? 32));
                this._resize = null;
                this._settings.set_int('width-percent', Math.max(15, Math.min(100, Math.round(w / wa.width * 100))));
                this._settings.set_int('lines', Math.max(1, Math.min(6, lines)));
            }
            this._drag = null;
            this._settings.set_int('floating-x', Math.round(x));
            this._settings.set_int('floating-y', Math.round(y));
            this._relayout();
            this._controls?.sync();
            return Clutter.EVENT_STOP;
        });
    }
}
