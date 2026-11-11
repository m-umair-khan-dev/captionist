// Hover controls for the caption window: ⋮ quick-settings menu, × close
// button, and a bottom-right resize grip.

import St from 'gi://St';
import Clutter from 'gi://Clutter';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import * as Slider from 'resource:///org/gnome/shell/ui/slider.js';

const TEXT_COLORS = [['#ffffff', 'White'], ['#ffd60a', 'Yellow'], ['#8ff0a4', 'Green'], ['#99c1f1', 'Blue'], ['#1c1c1c', 'Black']];
const BG_COLORS = [['#000000', 'Black'], ['#241f31', 'Dark'], ['#1a3a6b', 'Navy'], ['#f6f5f4', 'Light']];
const FONT_MIN = 14;
const FONT_MAX = 48;

function iconButton(iconName, accessibleName) {
    return new St.Button({
        style_class: 'captionist-button',
        child: new St.Icon({icon_name: iconName, style_class: 'captionist-button-icon'}),
        accessible_name: accessibleName,
        can_focus: true,
    });
}

/** A menu row: label on the left, widget filling the rest. */
function row(label, widget) {
    const item = new PopupMenu.PopupBaseMenuItem({activate: false, can_focus: false});
    const l = new St.Label({text: label, style_class: 'captionist-menu-label', y_align: Clutter.ActorAlign.CENTER});
    item.add_child(l);
    widget.x_expand = true;
    item.add_child(widget);
    return item;
}

export class CaptionControls {
    /**
     * @param {St.Widget} box caption window
     * @param {Gio.Settings} settings
     * @param {object} callbacks {onClose, onSettings, isResizing()}
     */
    constructor(box, settings, callbacks) {
        this._box = box;
        this._settings = settings;
        this._cb = callbacks;
        this._ids = [];

        // ── top-right buttons
        this._buttons = new St.BoxLayout({
            style_class: 'captionist-controls',
            x_expand: true,
            y_expand: true,
            x_align: Clutter.ActorAlign.END,
            y_align: Clutter.ActorAlign.START,
            visible: false,
        });
        this._more = iconButton('view-more-symbolic', 'Caption options');
        this._close = iconButton('window-close-symbolic', 'Turn off live captions');
        this._buttons.add_child(this._more);
        this._buttons.add_child(this._close);
        box.add_child(this._buttons);
        this._close.connect('clicked', () => this._cb.onClose());

        // ── resize grip (a visual hint: the caption window resizes from any
        // edge or corner, and pressing here resizes from this corner)
        this._grip = new St.Label({
            text: '◢',
            style_class: 'captionist-grip',
            reactive: true,
            x_expand: true,
            y_expand: true,
            x_align: Clutter.ActorAlign.END,
            y_align: Clutter.ActorAlign.END,
            visible: false,
        });
        box.add_child(this._grip);

        // ── quick menu
        this._menu = new PopupMenu.PopupMenu(this._more, 0.9, St.Side.TOP);
        this._menu.actor.add_style_class_name('captionist-menu');
        Main.uiGroup.add_child(this._menu.actor);
        this._menu.actor.hide();
        this._menuManager = new PopupMenu.PopupMenuManager(this._more);
        this._menuManager.addMenu(this._menu);
        this._buildMenu();
        this._more.connect('clicked', () => this._menu.toggle());
        this._menu.connect('open-state-changed', () => this.sync());

        this._ids.push([box, box.connect('notify::hover', () => this.sync())]);
    }

    destroy() {
        this._ids.forEach(([obj, id]) => obj.disconnect(id));
        this._ids = [];
        this._menu.destroy();
    }

    /** True while the user is interacting, so the window shouldn't auto-hide. */
    get busy() {
        return this._box.hover || this._menu.isOpen || this._cb.isResizing();
    }

    isGrip(actor) {
        return actor === this._grip || actor === this._grip.clutter_text;
    }

    sync() {
        const show = this._box.hover || this._menu.isOpen || this._cb.isResizing();
        this._buttons.visible = show;
        this._grip.visible = show;
    }

    // ── menu ──────────────────────────────────────────────────────────────

    _swatches(key, colors) {
        const box = new St.BoxLayout({style_class: 'captionist-swatches'});
        const buttons = colors.map(([hex, name]) => {
            const b = new St.Button({
                style_class: 'captionist-swatch',
                style: `background-color: ${hex};`,
                accessible_name: name,
                can_focus: true,
            });
            b._hex = hex;
            b.connect('clicked', () => this._settings.set_string(key, hex));
            box.add_child(b);
            return b;
        });
        const sync = () => {
            const cur = this._settings.get_string(key).toLowerCase();
            for (const b of buttons) {
                if (b._hex === cur)
                    b.add_style_pseudo_class('checked');
                else
                    b.remove_style_pseudo_class('checked');
            }
        };
        sync();
        this._ids.push([this._settings, this._settings.connect(`changed::${key}`, sync)]);
        return box;
    }

    _slider(key, toFraction, fromFraction) {
        const s = new Slider.Slider(toFraction(this._settings.get_int(key)));
        s.connect('notify::value', () => {
            const v = fromFraction(s.value);
            if (this._settings.get_int(key) !== v)
                this._settings.set_int(key, v);
        });
        this._ids.push([this._settings, this._settings.connect(`changed::${key}`, () => {
            const f = toFraction(this._settings.get_int(key));
            if (Math.abs(s.value - f) > 0.01)
                s.value = f;
        })]);
        return s;
    }

    _lines() {
        const box = new St.BoxLayout({style_class: 'captionist-stepper', x_align: Clutter.ActorAlign.END});
        const minus = new St.Button({label: '−', style_class: 'captionist-step button', can_focus: true});
        const value = new St.Label({style_class: 'captionist-step-value', y_align: Clutter.ActorAlign.CENTER});
        const plus = new St.Button({label: '+', style_class: 'captionist-step button', can_focus: true});
        const set = d => this._settings.set_int('lines', Math.max(1, Math.min(6, this._settings.get_int('lines') + d)));
        minus.connect('clicked', () => set(-1));
        plus.connect('clicked', () => set(1));
        const sync = () => {
            value.text = String(this._settings.get_int('lines'));
        };
        sync();
        this._ids.push([this._settings, this._settings.connect('changed::lines', sync)]);
        box.add_child(minus);
        box.add_child(value);
        box.add_child(plus);
        return box;
    }

    _buildMenu() {
        const m = this._menu;
        m.addMenuItem(row('Text colour', this._swatches('text-color', TEXT_COLORS)));
        m.addMenuItem(row('Background', this._swatches('bg-color', BG_COLORS)));
        m.addMenuItem(row('Opacity', this._slider('bg-opacity', v => v / 100, f => Math.round(f * 100))));
        m.addMenuItem(row('Text size', this._slider('font-size',
            v => (v - FONT_MIN) / (FONT_MAX - FONT_MIN),
            f => Math.round(FONT_MIN + f * (FONT_MAX - FONT_MIN)))));
        m.addMenuItem(row('Lines', this._lines()));
        m.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        m.addAction('Move to bottom', () => this._snap('bottom'));
        m.addAction('Move to top', () => this._snap('top'));
        m.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        m.addAction('All settings…', () => this._cb.onSettings());
        m.addAction('Turn off live captions', () => this._cb.onClose());
    }

    _snap(pos) {
        this._settings.set_string('position', pos);
        this._settings.set_int('floating-x', -1);
        this._settings.set_int('floating-y', -1);
    }
}
