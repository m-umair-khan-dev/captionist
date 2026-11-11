// Captionist Live Captions: GNOME Shell part.
// Draws the caption overlay, top-bar icon and Quick Settings toggle, and
// listens to the captionistd caption service over D-Bus. No audio or speech
// processing happens in GNOME Shell.

import GObject from 'gi://GObject';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {QuickToggle, SystemIndicator} from 'resource:///org/gnome/shell/ui/quickSettings.js';

import {CaptionOverlay} from './overlay.js';

const BUS_NAME = 'io.github.captionist.Daemon';
const OBJECT_PATH = '/io/github/captionist/Daemon';
const IFACE = 'io.github.captionist.Daemon1';
const APP_ID = 'io.github.captionist.Captionist.desktop';
/** Captionist's own symbolic icon, shipped in the extension (recoloured by GNOME). */
let ICON = null;
const iconFor = path => Gio.icon_new_for_string(`${path}/icons/captionist-symbolic.svg`);

const SOURCES = [['speakers', 'Speakers / headphones'], ['mic', 'Microphone'], ['both', 'Speakers + microphone']];
const POSITIONS = [['bottom', 'Bottom'], ['top', 'Top']];

function describeStatus(st) {
    const models = [st.live, st.refine].filter(Boolean).join(' + ');
    switch (st.state) {
    case 'listening': return {text: 'Listening', detail: models, cls: 'captionist-status-ok'};
    case 'loading': return {text: 'Loading…', detail: models, cls: ''};
    case 'setup-needed': return {text: 'Setup needed', detail: st.message, cls: 'captionist-status-warn'};
    case 'error': return {text: 'Problem', detail: st.message, cls: 'captionist-status-error'};
    default: return {text: 'Off', detail: '', cls: ''};
    }
}

const CaptionsToggle = GObject.registerClass(
class CaptionsToggle extends QuickToggle {
    constructor(settings) {
        super({title: 'Live Captions', gicon: ICON, toggleMode: true});
        settings.bind('enabled', this, 'checked', Gio.SettingsBindFlags.DEFAULT);
    }
});

const CaptionsQuickIndicator = GObject.registerClass(
class CaptionsQuickIndicator extends SystemIndicator {
    constructor(settings) {
        super();
        this.toggle = new CaptionsToggle(settings);
        this.quickSettingsItems.push(this.toggle);
    }

    destroy() {
        this.quickSettingsItems.forEach(i => i.destroy());
        super.destroy();
    }
});

const CaptionsButton = GObject.registerClass(
class CaptionsButton extends PanelMenu.Button {
    constructor(ext) {
        super(0.5, 'Captionist Live Captions');
        this._settings = ext.settings;
        this._ids = [];

        this._icon = new St.Icon({gicon: ICON, style_class: 'system-status-icon'});
        this.add_child(this._icon);

        this._switch = new PopupMenu.PopupSwitchMenuItem('Live captions', this._settings.get_boolean('enabled'));
        this._switch.connect('toggled', (_item, state) => this._settings.set_boolean('enabled', state));
        this.menu.addMenuItem(this._switch);

        this._statusItem = new PopupMenu.PopupMenuItem('', {reactive: false, can_focus: false});
        this._statusItem.label.add_style_class_name('captionist-status');
        this.menu.addMenuItem(this._statusItem);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._addSource(ext);
        this._addChoice('Position', 'position', POSITIONS);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this.menu.addAction('History…', () => ext.openSettings(['--history']));
        this.menu.addAction('Models…', () => ext.openSettings(['--models']));
        this.menu.addAction('Settings…', () => ext.openSettings([]));

        this._ids.push(this._settings.connect('changed::enabled', () => this._syncEnabled()));
        this._syncEnabled();
        this.setStatus({state: 'off'});
    }

    _syncEnabled() {
        const on = this._settings.get_boolean('enabled');
        this._switch.setToggleState(on);
        this._icon.opacity = on ? 255 : 140;
    }

    /** Source submenu: fixed sources, plus apps playing audio right now. */
    _addSource(ext) {
        const s = this._settings;
        const sub = new PopupMenu.PopupSubMenuMenuItem('Source');
        const fixed = SOURCES.map(([value, label]) => {
            const item = sub.menu.addAction(label, () => s.set_string('source', value));
            item._captionistValue = value;
            return item;
        });
        const appSection = new PopupMenu.PopupMenuSection();
        sub.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem('One app only'));
        sub.menu.addMenuItem(appSection);

        const sync = () => {
            const cur = s.get_string('source');
            const app = s.get_string('source-app');
            for (const it of fixed)
                it.setOrnament(it._captionistValue === cur ? PopupMenu.Ornament.CHECK : PopupMenu.Ornament.NONE);
            for (const it of appSection._getMenuItems())
                it.setOrnament(cur === 'app' && it._captionistApp === app ? PopupMenu.Ornament.CHECK : PopupMenu.Ornament.NONE);
            sub.label.text = cur === 'app'
                ? `Source: ${app || 'one app'}`
                : `Source: ${SOURCES.find(c => c[0] === cur)?.[1] ?? cur}`;
        };
        const fillApps = apps => {
            appSection.removeAll();
            const saved = s.get_string('source-app');
            if (saved && !apps.some(a => a.name === saved))
                apps.unshift({name: saved, media: 'not playing now'});
            if (apps.length === 0) {
                const none = new PopupMenu.PopupMenuItem('No app is playing audio', {reactive: false});
                none.label.add_style_class_name('captionist-status');
                appSection.addMenuItem(none);
            }
            for (const a of apps) {
                const label = a.media ? `${a.name} — ${a.media}` : a.name;
                const item = appSection.addAction(label.length > 48 ? `${label.slice(0, 47)}…` : label, () => {
                    s.set_string('source-app', a.name);
                    s.set_string('source', 'app');
                });
                item._captionistApp = a.name;
            }
            sync();
        };
        sub.menu.connect('open-state-changed', (_m, open) => {
            if (open)
                ext.listApps(fillApps);
        });
        sync();
        this._ids.push(s.connect('changed::source', sync));
        this._ids.push(s.connect('changed::source-app', sync));
        this.menu.addMenuItem(sub);
    }

    _addChoice(title, key, choices) {
        const sub = new PopupMenu.PopupSubMenuMenuItem(title);
        const items = choices.map(([value, label]) => {
            const item = sub.menu.addAction(label, () => {
                this._settings.set_string(key, value);
                if (key === 'position') {
                    // Snap a dragged caption window back to this edge.
                    this._settings.set_int('floating-x', -1);
                    this._settings.set_int('floating-y', -1);
                }
            });
            item._captionistValue = value;
            return item;
        });
        const sync = () => {
            const cur = this._settings.get_string(key);
            for (const it of items)
                it.setOrnament(it._captionistValue === cur ? PopupMenu.Ornament.CHECK : PopupMenu.Ornament.NONE);
            sub.label.text = `${title}: ${choices.find(c => c[0] === cur)?.[1] ?? cur}`;
        };
        sync();
        this._ids.push(this._settings.connect(`changed::${key}`, sync));
        this.menu.addMenuItem(sub);
    }

    setStatus(st) {
        const d = describeStatus(st);
        const label = this._statusItem.label;
        label.text = d.detail ? `${d.text} · ${d.detail}` : d.text;
        for (const c of ['captionist-status-ok', 'captionist-status-warn', 'captionist-status-error'])
            label.remove_style_class_name(c);
        if (d.cls)
            label.add_style_class_name(d.cls);
    }

    destroy() {
        this._ids.forEach(id => this._settings.disconnect(id));
        this._ids = [];
        super.destroy();
    }
});

export default class CaptionistExtension extends Extension {
    enable() {
        ICON = iconFor(this.path);
        this._watchForUpdate();
        this.settings = this.getSettings();
        this._overlay = new CaptionOverlay(this.settings, {
            onClose: () => this.settings.set_boolean('enabled', false),
            onSettings: () => this.openSettings(['--appearance']),
        });
        this._status = {state: 'off'};

        this._signalId = Gio.DBus.session.signal_subscribe(
            BUS_NAME, IFACE, null, OBJECT_PATH, null, Gio.DBusSignalFlags.NONE,
            (_conn, _sender, _path, _iface, signal, params) => this._onSignal(signal, params.deep_unpack()));

        this._settingIds = [
            this.settings.connect('changed::enabled', () => this._syncEnabled()),
            this.settings.connect('changed::show-indicator', () => this._syncIndicator()),
            this.settings.connect('changed::show-quick-settings', () => this._syncQuickSettings()),
        ];

        Main.wm.addKeybinding('toggle-shortcut', this.settings, Meta.KeyBindingFlags.NONE,
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW,
            () => this.settings.set_boolean('enabled', !this.settings.get_boolean('enabled')));

        this._syncIndicator();
        this._syncQuickSettings();
        this._syncEnabled();
    }

    disable() {
        this._updateMonitor?.cancel();
        this._updateMonitor = null;
        Main.wm.removeKeybinding('toggle-shortcut');
        Gio.DBus.session.signal_unsubscribe(this._signalId);
        this._settingIds.forEach(id => this.settings.disconnect(id));
        this._button?.destroy();
        this._button = null;
        this._quick?.destroy();
        this._quick = null;
        this._overlay.destroy();
        this._overlay = null;
        this.settings = null;
    }

    _syncIndicator() {
        const want = this.settings.get_boolean('show-indicator');
        if (want && !this._button) {
            this._button = new CaptionsButton(this);
            this._button.setStatus(this._status);
            Main.panel.addToStatusArea('captionist', this._button);
        } else if (!want && this._button) {
            this._button.destroy();
            this._button = null;
        }
    }

    _syncQuickSettings() {
        const want = this.settings.get_boolean('show-quick-settings');
        if (want && !this._quick) {
            this._quick = new CaptionsQuickIndicator(this.settings);
            this._quick.toggle.subtitle = describeStatus(this._status).text;
            Main.panel.statusArea.quickSettings.addExternalIndicator(this._quick);
        } else if (!want && this._quick) {
            this._quick.destroy();
            this._quick = null;
        }
    }

    _syncEnabled() {
        const on = this.settings.get_boolean('enabled');
        this._overlay.setEnabled(on);
        if (on) {
            this._overlay.setMessage('Starting live captions…');
            this._askStatus();
        } else {
            this._onStatus({state: 'off'});
        }
    }

    /** Calling the service also starts it (D-Bus activation). */
    _askStatus() {
        Gio.DBus.session.call(BUS_NAME, OBJECT_PATH, IFACE, 'Status', null,
            new GLib.VariantType('(s)'), Gio.DBusCallFlags.NONE, 20000, null,
            (conn, res) => {
                try {
                    const [json] = conn.call_finish(res).deep_unpack();
                    this._onStatus(JSON.parse(json));
                } catch (e) {
                    console.error(`Captionist: caption service not reachable: ${e.message}`);
                    this._onStatus({state: 'error', message: 'Caption service not available. Is the Captionist app installed?'});
                }
            });
    }

    _onSignal(name, args) {
        if (!this._overlay)
            return;
        try {
            switch (name) {
            case 'Partial':
                this._overlay.partial(args[0], args[1]);
                break;
            case 'Final':
                this._overlay.final(args[0], args[1]);
                break;
            case 'StatusChanged':
                this._onStatus(JSON.parse(args[0]));
                break;
            }
        } catch (e) {
            this._reportError(`signal ${name}`, e);
        }
    }

    /**
     * Logs an error and passes it to the caption service, which keeps it for
     * "Report a problem" and sends it only if the user turned crash reports on.
     * Never includes caption text: only the error and where it happened.
     */
    _reportError(place, e) {
        console.error(`Captionist: ${place}: ${e}`);
        const stack = String(e?.stack ?? '').split('\n').slice(0, 8).join('\n');
        Gio.DBus.session.call(BUS_NAME, OBJECT_PATH, IFACE, 'ReportExtensionError',
            new GLib.Variant('(ss)', [place, `${e?.message ?? e}\n${stack}`]),
            null, Gio.DBusCallFlags.NO_AUTO_START, 5000, null, null);
    }

    _onStatus(st) {
        this._status = st;
        this._button?.setStatus(st);
        if (this._quick)
            this._quick.toggle.subtitle = st.state === 'off' ? '' : describeStatus(st).text;
        if (!this.settings?.get_boolean('enabled'))
            return;
        switch (st.state) {
        case 'loading':
            this._overlay.setMessage('Loading speech models…');
            break;
        case 'listening':
            this._overlay.setMessage('Listening…');
            break;
        case 'setup-needed':
            this._overlay.setMessage(`Captionist: ${st.message}`, true);
            break;
        case 'error':
            this._overlay.setMessage(`Captionist: ${st.message}`, true);
            break;
        }
    }

    /**
     * GNOME keeps running the old extension code after a package update until
     * the next login. Watch our own folder and tell the user once when a new
     * version has been installed.
     */
    _watchForUpdate() {
        const running = this.metadata['version-name'] ?? '';
        const dir = Gio.File.new_for_path(this.path);
        this._updateMonitor = dir.monitor_directory(Gio.FileMonitorFlags.WATCH_MOVES, null);
        this._updateMonitor.connect('changed', (_m, file) => {
            if (this._updateNotified || file?.get_basename() !== 'metadata.json')
                return;
            try {
                const [, bytes] = dir.get_child('metadata.json').load_contents(null);
                const installed = JSON.parse(new TextDecoder().decode(bytes))['version-name'] ?? '';
                if (installed && installed !== running) {
                    this._updateNotified = true;
                    Main.notify('Captionist was updated',
                        `Version ${installed} is installed. Log out and back in to finish updating.`);
                }
            } catch {
                // Mid-update the file may be missing or partial; a later event will retry.
            }
        });
    }

    /** Calls back with apps playing audio: [{name, media, icon}]. */
    listApps(callback) {
        Gio.DBus.session.call(BUS_NAME, OBJECT_PATH, IFACE, 'ListApps', null,
            new GLib.VariantType('(s)'), Gio.DBusCallFlags.NONE, 5000, null,
            (conn, res) => {
                let apps = [];
                try {
                    apps = JSON.parse(conn.call_finish(res).deep_unpack()[0]);
                } catch {}
                callback(apps);
            });
    }

    openSettings(args) {
        try {
            const app = Gio.DesktopAppInfo.new(APP_ID);
            const exe = app?.get_executable() ?? 'captionist';
            Gio.Subprocess.new([exe, ...args], Gio.SubprocessFlags.NONE);
        } catch (e) {
            Main.notify('Captionist', `Could not open settings: ${e.message}`);
        }
    }
}
