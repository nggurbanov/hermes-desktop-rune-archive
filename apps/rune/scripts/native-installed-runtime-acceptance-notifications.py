#!/usr/bin/env python3
"""Private notification daemon fixture. Run only on an explicitly supplied private bus."""
import json
import sys
from gi.repository import Gio, GLib

if len(sys.argv) != 2 or not sys.argv[1].startswith('unix:'):
    raise SystemExit('usage: fixture PRIVATE_UNIX_BUS_ADDRESS')
address = sys.argv[1]
connection = Gio.DBusConnection.new_for_address_sync(address, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
name_reply = connection.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'RequestName', GLib.Variant('(su)', ('org.freedesktop.Notifications', 4)), GLib.VariantType.new('(u)'), Gio.DBusCallFlags.NONE, -1, None)
if name_reply.unpack()[0] != 1:
    raise SystemExit('private notification name already owned')
xml = '''<node><interface name="org.freedesktop.Notifications">
<method name="GetCapabilities"><arg type="as" direction="out"/></method>
<method name="GetServerInformation"><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/></method>
<method name="Notify"><arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/><arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/></method>
<method name="CloseNotification"><arg type="u" direction="in"/></method>
<signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal><signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal>
</interface></node>'''
interface = Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0]
next_id = 1
ids = set()
def log(value):
    print(json.dumps(value, ensure_ascii=False), flush=True)
def method(conn, sender, obj, iface, name, params, invocation):
    global next_id
    if name == 'GetCapabilities': invocation.return_value(GLib.Variant('(as)', (['actions'],)))
    elif name == 'GetServerInformation': invocation.return_value(GLib.Variant('(ssss)', ('Fixture', 'private', '1', '1.2')))
    elif name == 'Notify':
        app, replace, icon, title, body, actions, hints, timeout = params.unpack()
        current = next_id; next_id += 1; ids.add(current)
        log({'event':'notify', 'id':current, 'app':app, 'title':title, 'body':body, 'actions':actions, 'hints':hints})
        invocation.return_value(GLib.Variant('(u)', (current,)))
    elif name == 'CloseNotification':
        current, = params.unpack(); ids.discard(current); log({'event':'close-request', 'id':current}); invocation.return_value(None)
connection.register_object('/org/freedesktop/Notifications', interface, method, None, None)
loop = GLib.MainLoop()
def command(source, condition):
    line = sys.stdin.readline()
    if not line: loop.quit(); return False
    try:
        request = json.loads(line); current = request['id']
        if current not in ids: raise ValueError('unknown notification id')
        if request['event'] == 'action':
            action = request['action']; connection.emit_signal(None, '/org/freedesktop/Notifications', 'org.freedesktop.Notifications', 'ActionInvoked', GLib.Variant('(us)', (current, action)))
        elif request['event'] == 'closed':
            connection.emit_signal(None, '/org/freedesktop/Notifications', 'org.freedesktop.Notifications', 'NotificationClosed', GLib.Variant('(uu)', (current, request.get('reason', 2)))); ids.remove(current)
        else: raise ValueError('unknown event')
        log({'event':'emitted', 'request':request})
    except (ValueError, KeyError, TypeError) as error: log({'event':'error', 'message':str(error)})
    return True
GLib.io_add_watch(sys.stdin, GLib.IO_IN | GLib.IO_HUP, command)
log({'event':'ready'})
loop.run()
