#!/usr/bin/env python3
# A fake MPRIS music player ("Fake Spotify") for testing Wallplayer's Linux now-playing support.
import dbus, dbus.service, dbus.mainloop.glib, sys, time
from gi.repository import GLib

dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
IFACE = 'org.mpris.MediaPlayer2.Player'
LOG = open('/tmp/fake_mpris.log', 'a', buffering=1)

class Player(dbus.service.Object):
    def __init__(self, bus):
        super().__init__(bus, '/org/mpris/MediaPlayer2')
        self.status = 'Playing'; self.shuffle = False; self.t0 = time.time(); self.pos0 = 42_000_000
        self.idx = 0
        self.tracks = [('Nàng Thơ', ['Hoàng Dũng'], 'Single', 254_000_000), ('Có Chàng Trai Viết Lên Cây', ['Phan Mạnh Quỳnh'], 'Mắt Biếc OST', 300_000_000)]
    def pos(self):
        return int(self.pos0 + ((time.time() - self.t0) * 1e6 if self.status == 'Playing' else 0))
    def meta(self):
        t, a, al, ln = self.tracks[self.idx]
        return dbus.Dictionary({'mpris:trackid': dbus.ObjectPath('/fake/track/%d' % self.idx), 'xesam:title': t,
            'xesam:artist': dbus.Array(a, signature='s'), 'xesam:album': al, 'mpris:length': dbus.Int64(ln),
            'mpris:artUrl': 'file:///tmp/wp-test-cover.png'}, signature='sv')
    @dbus.service.method('org.freedesktop.DBus.Properties', in_signature='ss', out_signature='v')
    def Get(self, iface, prop):
        return self.GetAll(iface)[prop]
    @dbus.service.method('org.freedesktop.DBus.Properties', in_signature='s', out_signature='a{sv}')
    def GetAll(self, iface):
        if iface == 'org.mpris.MediaPlayer2':
            return {'Identity': 'Fake Spotify', 'CanQuit': False, 'CanRaise': False}
        return {'PlaybackStatus': self.status, 'Metadata': self.meta(), 'Position': dbus.Int64(self.pos()),
                'Shuffle': self.shuffle, 'CanPlay': True, 'CanPause': True, 'CanSeek': True}
    @dbus.service.method('org.freedesktop.DBus.Properties', in_signature='ssv')
    def Set(self, iface, prop, val):
        LOG.write('Set %s=%s\n' % (prop, val));
        if prop == 'Shuffle': self.shuffle = bool(val)
    def _rebase(self):
        self.pos0 = self.pos(); self.t0 = time.time()
    @dbus.service.method(IFACE)
    def PlayPause(self):
        self._rebase(); self.status = 'Paused' if self.status == 'Playing' else 'Playing'; LOG.write('PlayPause -> %s\n' % self.status)
    @dbus.service.method(IFACE)
    def Play(self): self._rebase(); self.status = 'Playing'; LOG.write('Play\n')
    @dbus.service.method(IFACE)
    def Pause(self): self._rebase(); self.status = 'Paused'; LOG.write('Pause\n')
    @dbus.service.method(IFACE)
    def Next(self): self.idx = (self.idx + 1) % 2; self.pos0 = 0; self.t0 = time.time(); LOG.write('Next\n')
    @dbus.service.method(IFACE)
    def Previous(self): self.idx = (self.idx - 1) % 2; self.pos0 = 0; self.t0 = time.time(); LOG.write('Previous\n')
    @dbus.service.method(IFACE, in_signature='ox')
    def SetPosition(self, track, pos): self.pos0 = int(pos); self.t0 = time.time(); LOG.write('SetPosition %s %d\n' % (track, pos))
    @dbus.service.method(IFACE, in_signature='x')
    def Seek(self, off): self.pos0 = self.pos() + int(off); self.t0 = time.time(); LOG.write('Seek %d\n' % off)

bus = dbus.SessionBus()
name = dbus.service.BusName('org.mpris.MediaPlayer2.fakespotify', bus)
p = Player(bus)
LOG.write('fake player up\n')
GLib.MainLoop().run()
