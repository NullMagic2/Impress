"""Check live Save dialog suffixes on an isolated GTK display.

Requires python3-gi (AT-SPI), xdotool, xfwm4 and Pillow. Run on a disposable
X server, never the user's desktop:
  dbus-run-session -- xvfb-run -a python3 tools/save_dialog_smoke.py \
      /absolute/path/to/Impress --output /tmp/impress-save-dialog-results
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile
import time
import zipfile

import gi
gi.require_version('Atspi', '2.0')
from gi.repository import Atspi, GLib
from PIL import ImageGrab

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('executable', type=Path)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
exe = args.executable.resolve()
qa = args.output.resolve()
qa.mkdir(parents=True, exist_ok=True)


def key(*keys):
    subprocess.run(['xdotool', 'key', *keys], check=True, timeout=10)
    time.sleep(.15)


def walk(element):
    try:
        yield element
        for i in range(element.get_child_count()):
            child = element.get_child_at_index(i)
            if child:
                yield from walk(child)
    except GLib.Error:
        return


def eventually(callback):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        if app.poll() is not None:
            raise AssertionError('Impress exited; see gui.log')
        result = callback()
        if result:
            return result
        time.sleep(.1)
    raise AssertionError('Timed out waiting for the Save dialog')


Atspi.init()
with tempfile.TemporaryDirectory(prefix='impress-save-dialog-') as tmp:
    tmp = Path(tmp)
    env = {**os.environ, 'GDK_BACKEND': 'x11', 'GTK_USE_PORTAL': '0', 'LANGUAGE': 'en'}
    for setting in ('XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_DATA_HOME', 'IMPRESS_RECOVERY_DIR'):
        env[setting] = str(tmp / setting)
    wm = subprocess.Popen(['xfwm4', '--compositor=off'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    log = (qa / 'gui.log').open('w')
    app = subprocess.Popen([exe], env=env, stdout=log, stderr=subprocess.STDOUT)

    def elements():
        desktop = Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            application = desktop.get_child_at_index(i)
            if application.get_process_id() == app.pid:
                yield from walk(application)

    def find(role, name):
        return next((e for e in elements() if e.get_role_name() == role and e.get_name() == name), None)

    def filename(expected):
        return next((e for e in elements() if e.get_role_name() == 'text'
                     and Atspi.Text.get_text(e, 0, -1) == expected), None)

    try:
        def window():
            result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--class', '^Impress$'],
                                    capture_output=True, text=True)
            return result.stdout.strip().splitlines() if result.returncode == 0 else None
        windows = eventually(window)
        subprocess.run(['xdotool', 'windowactivate', '--sync', windows[0]], check=True)
        time.sleep(1)
        key('ctrl+shift+s')
        entry = eventually(lambda: filename('Untitled.impress'))
        combo = eventually(lambda: find('combo box', 'Impress documents (*.impress)'))
        embed = eventually(lambda: find('check box', 'Embed fonts'))
        # Exercise each filter repeatedly, preserving a Unicode basename with dots.
        base = 'Έκθεση.v2'
        assert Atspi.EditableText.set_text_contents(entry, base + '.impress')

        def choose(index, expected):
            bounds = Atspi.Component.get_extents(combo, Atspi.CoordType.SCREEN)
            subprocess.run(['xdotool', 'mousemove', str(bounds.x + bounds.width // 2),
                            str(bounds.y + bounds.height // 2), 'click', '1'], check=True)
            key('Home', *(['Down'] * index), 'Return')
            eventually(lambda: Atspi.Text.get_text(entry, 0, -1) == expected)
            assert embed.get_state_set().contains(Atspi.StateType.SENSITIVE) == (index in (0, 1))
            print('Visible filename:', expected, flush=True)

        for index, ext in [(1, 'docx'), (2, 'typ'), (3, 'rtf'), (4, 'doc'), (0, 'impress'), (1, 'docx')]:
            choose(index, base + '.' + ext)
        assert Atspi.EditableText.set_text_contents(entry, 'New name')
        choose(3, 'New name.rtf')
        assert Atspi.EditableText.set_text_contents(entry, '')
        choose(0, '')
        assert Atspi.EditableText.set_text_contents(entry, 'Custom.name.DOCX')
        choose(1, 'Custom.name.DOCX')
        # Save the chosen format, then reopen Save As and cancel another switch.
        destination = tmp / (base + '.docx')
        assert Atspi.EditableText.set_text_contents(entry, str(destination))
        ImageGrab.grab(xdisplay=os.environ['DISPLAY']).save(qa / 'docx-selected.png')
        key('alt+s')
        eventually(destination.exists)
        with zipfile.ZipFile(destination) as saved:
            assert 'word/document.xml' in saved.namelist()
        assert not destination.with_suffix('.impress').exists()
        time.sleep(.5)
        key('ctrl+shift+s')
        entry = eventually(lambda: filename(destination.name))
        combo = eventually(lambda: find('combo box', 'Microsoft Word documents (*.docx)'))
        embed = eventually(lambda: find('check box', 'Embed fonts'))
        choose(0, base + '.impress')
        key('Escape')
        assert not destination.with_suffix('.impress').exists()
        key('ctrl+shift+s')
        eventually(lambda: filename(destination.name))
        key('Escape')
        print('DOCX save, reopen and cancel passed.', flush=True)
    except Exception:
        ImageGrab.grab(xdisplay=os.environ['DISPLAY']).save(qa / 'failure.png')
        raise
    finally:
        app.terminate()
        app.wait(timeout=10)
        wm.terminate()
        wm.wait(timeout=10)
        log.close()
