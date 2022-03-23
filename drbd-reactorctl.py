#!/usr/bin/env python3

import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request

DEFAULT_SNIPPETS = '/etc/drbd-reactor.d'
REACTOR_SERVICE = 'drbd-reactor.service'
REACTOR_RELOAD_PATH = 'drbd-reactor-reload.path'
BLACK, RED, GREEN, YELLOW, BLUE, MAGENTA, CYAN, WHITE = list(range(8))
PLUGIN_TYPES = ('promoter', 'umh', 'debugger', 'prometheus')


def eprint(*args, **kwargs):
    print(*args, file=sys.stderr, **kwargs)


def die(*args, **kwargs):
    eprint(*args, **kwargs)
    sys.exit(1)


try:
    import toml
except Exception:
    die('Could not import toml library:\n',
        '- On Debian based systems install "python3-toml".\n',
        '- On RHEL7 install "epel-release and python36-toml".\n',
        '- On RHEL8 install "epel-release and python3-toml".\n',
        '- On SLES >= 15 install "python3-toml".',
        )


def has_colors(stream):
    if not hasattr(stream, "isatty"):
        return False
    if not stream.isatty():
        return False  # auto color only on TTYs
    try:
        import curses
        curses.setupterm()
        return curses.tigetnum("colors") > 2
    except Exception:
        # guess false in case of error
        return False

    return True


def color_string(text, color=WHITE, stream=sys.stdout):
    if has_colors(stream):
        return "\x1b[1;{0}m{1}\x1b[0m".format(30+color, text)
    return text


class Plugin(object):
    @staticmethod
    def new(content, cfg_file=''):
        # here content is the content of a file (as dict), which could contain multiple plugins
        plugins = []
        for promoter in content.get('promoter', []):
            plugins.append(Promoter(promoter, cfg_file))
        for prometheus in content.get('prometheus', []):
            plugins.append(Prometheus(prometheus, cfg_file))
        for umh in content.get('umh', []):
            plugins.append(UMH(umh, cfg_file))
        for debugger in content.get('debugger', []):
            plugins.append(Debugger(debugger, cfg_file))

        return plugins

    @classmethod
    def from_files(cls, files):
        plugins = []
        for pf in files:
            p = cls.new(toml.load(pf), pf)
            if p:
                plugins += p
        return plugins

    @staticmethod
    def new_template(ptype):
        if ptype == 'promoter':
            return Promoter.template()
        elif ptype == 'prometheus':
            return Prometheus.template()
        elif ptype == 'umh':
            return UMH.template()
        elif ptype == 'debugger':
            return Debugger.template()

    def __init__(self, config, cfg_file=''):
        self._config = config
        self._cfg_file = cfg_file

    @property
    def id(self):
        return self._config.get('id', '<none>')

    @property
    def targets(self):
        return []

    @property
    def header(self):
        return ''

    def show_status(self, verbose=False):
        if verbose:
            print(self.cfg_file + ':')

    @property
    def cfg_file(self):
        return self._cfg_file


class Prometheus(Plugin):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    @staticmethod
    def template():
        return """[[prometheus]]
id = "prometheus"  # usually there is only one, this id should be fine
enums = true
# address = "0.0.0.0:9942" """

    @property
    def header(self):
        return "Prometheus (ID: '{}')".format(self.id)

    def show_status(self, verbose=False):
        super().show_status(verbose)
        address = self._config.get('address', '0.0.0.0:9942')
        header = '{} listening on {}'.format(self.header, address)
        print(color_string(header, color=GREEN))
        if verbose:
            get = color_string('successful', color=GREEN)
            try:
                urllib.request.urlopen('http://' + address, timeout=2).read()
            except Exception:
                get = color_string('failed', color=RED)

            print('HTTP GET: {}'.format(get))


class Promoter(Plugin):
    UNKNOWN = '<unknown>'

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    @staticmethod
    def template():
        return """[[promoter]]
id = "${id}"
[promoter.resources.${resname}]
start = ["${service.mount}", "${service.service}"]
# runner = systemd
## if unset/empty, services from 'start' will be stopped in reverse order if runner is shell
## if runner is sytemd it just stops the implicitly generated systemd.target
# stop = []
# on-drbd-demote-failure = "reboot"
# stop-services-on-exit = false
#
# for more complex setups like HA iSCSI targets, NFS exports, or NVMe-oF targets consider
# https://github.com/LINBIT/linstor-gateway which uses LINSTOR and drbd-reactor"""

    @property
    def header(self):
        return "Promoter (ID: '{}')".format(self.id)

    @staticmethod
    def target_name(name):
        return 'drbd-services@{}.target'.format(systemd_escape_name(name))

    def _get_res_names(self):
        return [name for (name, options) in
                self._config.get('resources', {}).items() if
                options.get('runner', 'systemd') == 'systemd']

    def _get_start(self, name):
        return self._config.get('resources', {}).get(name, {}).get('start', [])

    def _get_primary_on(self, name):
        try:
            out = subprocess.run(['drbdsetup', 'status', '--json', name],
                                 check=True, stdout=subprocess.PIPE).stdout
            out = json.loads(out)[0]  # always a single res
        except Exception:
            return self.UNKNOWN

        # is it me?
        if out.get('role') == 'Primary':
            return socket.gethostname()
        # one of the peers?
        for con in out.get('connections', []):
            if con.get('peer-role') == 'Primary':
                return con.get('name', self.UNKNOWN)
        return self.UNKNOWN

    def show_status(self, verbose=False):
        super().show_status(verbose)
        print(color_string(self.header, color=GREEN))

        for name in self._get_res_names():
            primary = self._get_primary_on(name)
            if primary == socket.gethostname():
                primary = 'this node'
            else:
                primary = 'node "{}"'.format(primary)
            print('Most likely active on {}'.format(primary))

            target = Promoter.target_name(name)
            if verbose:
                systemctl('status', '--no-pager', target)
                systemctl('status', '--no-pager', 'drbd-promote@{}.service'.format(systemd_escape_name(name)))
                for service in self._get_start(name):
                    service = service.strip()
                    if service.startswith('ocf:'):
                        ra = service.split()
                        if len(ra) < 2:
                            eprint("could not parse ocf service ('{}')".format(service))
                            continue
                        escaped_service_template = systemd_escape_name('{}_{}'.format(ra[1], name))
                        service = 'ocf.ra@{}.service'.format(escaped_service_template)
                    systemctl('status', '--no-pager', service)
            else:
                systemctl('list-dependencies', '--no-pager', target)

    @property
    def targets(self):
        return [Promoter.target_name(name) for name in self._get_res_names()]


class UMH(Plugin):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    @staticmethod
    def template():
        return """[[umh]]
id = "${id}"
[[umh.resource]]
command = "slack.sh $DRBD_RES_NAME on $(uname -n) from $DRBD_OLD_ROLE to $DRBD_NEW_ROLE"
event-type = "Change"
old.role = { operator = "NotEquals", value = "Primary" }
new.role = "Primary"
# This is a trivial resource rule example, please see drbd-reactor.umh(5) for more examples"""

    @property
    def header(self):
        return "UMH (ID: '{}')".format(self.id)

    def show_status(self, verbose=False):
        super().show_status(verbose)
        header = '{} {}'.format(self.header, 'started')
        print(color_string(header, color=GREEN))


class Debugger(Plugin):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    @staticmethod
    def template():
        return """[[debugger]]
id = "debugger"  # usually there is only one, id should be fine
# NOTE: make sure the log level in your [[log]] section is at least on level 'debug' """

    @property
    def header(self):
        return "Debugger (ID: '{}')".format(self.id)

    def show_status(self, verbose=False):
        super().show_status(verbose)
        # TODO: check loggers for at least debug level
        header = '{} {}'.format(self.header, 'started')
        print(color_string(header, color=GREEN))


def fdisable(name):
    return name + '.disabled'


def fenable(name):
    if not name.endswith('.disabled'):
        raise Exception('Can not enable file that does not end with .disabled')

    return name[:-len('.disabled')]


def systemctl(*args):
    what = ['systemctl'] + list(args)
    # eprint(what)
    env = os.environ.copy()
    env['SYSTEMD_COLORS'] = str(int(has_colors(sys.stdout)))
    out = subprocess.run(what, env=env, stdout=subprocess.PIPE).stdout.decode()
    if out:
        print(out)


def systemd_escape_name(name):
    return subprocess.run(['systemd-escape', '--', name], check=True,
                          stdout=subprocess.PIPE).stdout.decode().strip()


def reload_service():
    systemctl('reload', REACTOR_SERVICE)


def has_autoload():
    return subprocess.run(('systemctl', 'is-active', '-q', REACTOR_RELOAD_PATH)).returncode == 0


def get_plugin_files(config, plugins, ext='.toml'):
    plugin_files = []

    config = toml.load(config)
    snippets = config.get('snippets', DEFAULT_SNIPPETS)

    if len(plugins) == 0:  # get all of them
        for f in os.listdir(snippets):
            if f.endswith(ext):
                plugins.append(f[:-len(ext)])

    for plugin in plugins:
        path = os.path.join(snippets, plugin + ext)
        if not os.path.exists(path):
            eprint('{} does not exist, ignoring'.format(path))
            continue

        plugin_files.append(path)

    return plugin_files


def status(args):
    files = get_plugin_files(args.config, args.configs) + [args.config]
    verbose = getattr(args, 'verbose', False)

    for p in Plugin.from_files(files):
        p.show_status(verbose)


def ls(args):
    files = []
    if args.disabled:
        files = get_plugin_files(args.config, args.configs, ext='.toml.disabled')
    else:
        files = get_plugin_files(args.config, args.configs) + [args.config]

    color = RED if args.disabled else GREEN
    for p in Plugin.from_files(files):
        print(p.cfg_file)
        print(color_string(p.header, color=color))


def cat(args):
    catter = 'cat'

    for util in ('bat', 'batcat'):
        if shutil.which(util):
            catter = util
            break

    for pf in get_plugin_files(args.config, args.configs):
        eprint('{}:'.format(pf))
        os.system('{} {}'.format(catter, pf))


def _disable_files(plugin_files, now):
    for pf in plugin_files:
        os.rename(pf, fdisable(pf))

    # we have to keep this order
    # reload first, so that a stop does not trigger a start again
    if len(plugin_files) > 0 and not has_autoload():
        reload_service()

    if now:
        for pf in plugin_files:
            content = toml.load(fdisable(pf))
            for plugin in Plugin.new(content):
                for target in plugin.targets:
                    systemctl('stop', target)

    return len(plugin_files)


def disable(args):
    plugin_files = get_plugin_files(args.config, args.configs)
    return _disable_files(plugin_files, args.now)


def _enable_files(plugin_files):
    for pf in plugin_files:
        os.rename(pf, fenable(pf))
    nr_plugins = len(plugin_files)
    if nr_plugins > 0 and not has_autoload():
        reload_service()
    return nr_plugins


def enable(args):
    plugin_files = get_plugin_files(args.config, args.configs, ext='.toml.disabled')
    return _enable_files(plugin_files)


def restart_files(plugin_files):
    for pf in plugin_files:
        print('Restarting {}'.format(pf))
        os.rename(pf, fdisable(pf))
    if not has_autoload():
        reload_service()
    for pf in plugin_files:
        os.rename(fdisable(pf), pf)
    if not has_autoload():
        reload_service()


def restart(args):
    plugin_files = get_plugin_files(args.config, args.configs)
    if len(plugin_files) == 0:
        systemctl('restart', REACTOR_SERVICE)
        return 0

    restart_files(plugin_files)

    if args.with_targets:
        for plugin in Plugin.from_files(plugin_files):
            for target in plugin.targets:
                systemctl('restart', target)

    return len(plugin_files)


def ask(what, force=False, default=False):
    if force:
        return True

    default_str = '[Y/n]' if default else '[N/y]'
    while True:
        got = input('{} {} '.format(what, default_str)).lower()
        if got == '':
            if default:
                got = 'y'
            else:
                got = 'n'
        if got in ('no', 'n'):
            return False
        if got in ('yes', 'y'):
            return True


def remove(args):
    ext = '.toml.disabled' if args.disabled else '.toml'
    plugin_files = get_plugin_files(args.config, args.configs, ext=ext)
    nr_removed = 0
    for pf in plugin_files:
        if ask("Remove '{}'?".format(pf), force=args.force):
            os.remove(pf)
            nr_removed += 1
    if nr_removed > 0 and not has_autoload():
        reload_service()
    return nr_removed


def edit(args):
    config = toml.load(args.config)
    snippets = config.get('snippets', DEFAULT_SNIPPETS)

    plugin_name = args.configs[0]
    config_file = plugin_name + '.toml'  # single file enforced by argparse
    final_file = os.path.join(snippets, config_file)
    if not os.path.exists(final_file):
        # maybe disabled?
        disabled = fdisable(final_file)
        if os.path.exists(disabled):
            final_file = disabled
        # else we keep the orinal and populate it

    tmp_file = tempfile.NamedTemporaryFile()

    final_exists = os.path.exists(final_file)
    if final_exists:
        shutil.copy(final_file, tmp_file.name)
    else:
        template = Plugin.new_template(args.type)
        template = template.replace('${id}', plugin_name)
        with open(tmp_file.name, 'w') as f:
            f.write(template + '\n')

    editor = os.environ.get('EDITOR', 'vi')
    os.system('{} {}'.format(editor, tmp_file.name))

    toml_valid = True
    try:
        toml.load(tmp_file.name)
    except Exception as e:
        toml_valid = False
        eprint('toml snippet not valid ({}), bye'.format(e))
    finally:
        if toml_valid:
            shutil.copy(tmp_file.name, final_file)
        tmp_file.close()
        if not toml_valid:
            sys.exit(1)

    if final_file.endswith('.disabled'):
        print(color_string('NOTE:', color=YELLOW),
              'Disabled file ({}) is not enabled automatically, use "enable" subcommand'.format(final_file))
        return 0

    if not has_autoload():
        if final_exists:
            restart_files([final_file])
        else:
            reload_service()

    print(color_string('INFO:', color=GREEN),
          'Please make sure to copy to {} to all other cluster nodes '
          'and execute "systemctl reload drbd-reactor.service"'.format(final_file))


def evict(args):
    files = get_plugin_files(args.config, args.configs)
    if not args.force:  # sanity checks
        for f in files:
            ps = Plugin.from_files([f])
            if len(ps) != 1:
                die('Config file {} contains multiple plugins'.format(f))
            for p in ps:
                if isinstance(p, Promoter) and len(p._get_res_names()) != 1:
                    die('Promoter in config file {} responsible for multiple resources'.format(f))

    me = socket.gethostname()
    for p in Plugin.from_files(files):
        if not isinstance(p, Promoter):
            continue
        for name in p._get_res_names():
            print(color_string('Evicting {}'.format(name), color=GREEN))
            primary = p._get_primary_on(name)
            if primary == Promoter.UNKNOWN:
                print('Sorry, resource state for "{}" unknown, ignoring'.format(name))
                continue
            if primary != me:
                print('Active on {}, nothing to do on this node, ignoring'.format(primary))
                continue
            cfg_file = p.cfg_file
            if not cfg_file:
                raise Exception('Promoter for {} has no config file'.format(name))

            try:
                _disable_files([cfg_file], now=True)

                needs_newline = False
                for i in range(args.delay, -1, -1):  # -1 to really give it the full time
                    primary = p._get_primary_on(name)
                    if primary != Promoter.UNKNOWN and primary != me:  # a know host/peer
                        break

                    s = str(i) + '..' if i != 0 else str(i)
                    print(s, sep='', end='')
                    sys.stdout.flush()
                    needs_newline = True
                    if i != 0:  # no need to sleep on last iteration
                        time.sleep(1)

                if needs_newline:  # finish with newline
                    print()

                if primary == Promoter.UNKNOWN:
                    print('Unfortunately no other node took over, resource in unknown state')
                elif primary == me:
                    print('Unfortunately no other node took over, local node still DRBD Primary')
                else:
                    print('Node {} took over'.format(primary))
            except KeyboardInterrupt:
                print('\ninterrupted')
            finally:
                print('Re-enabling the config')
                _enable_files([fdisable(cfg_file)])


def main():
    parser = argparse.ArgumentParser(prog='drbd-reactorctl')
    parser.add_argument('-c', '--config', default='/etc/drbd-reactor.toml',
                        help='path to the toml config file')
    parser.add_argument('--color', default='auto', choices=('auto', 'always', 'never'),
                        help='color output')
    parser.set_defaults(func=status)
    parser.set_defaults(configs=[])

    subparsers = parser.add_subparsers(help='sub-command help')

    parser_disable = subparsers.add_parser('disable', help='disable drbd-reactor configs')
    parser_disable.set_defaults(func=disable)
    parser_disable.add_argument('--now', action='store_true',
                                help='in case of promoter plugins stop the drbd-resources target')
    parser_disable.add_argument('configs', nargs='*',
                                help='configs to disable')

    parser_enable = subparsers.add_parser('enable', help='enable drbd-reactor configs')
    parser_enable.set_defaults(func=enable)
    parser_enable.add_argument('configs', nargs='*',
                               help='configs to disable')

    parser_status = subparsers.add_parser('status', help='plugin status')
    parser_status.set_defaults(func=status)
    parser_status.add_argument('-v', '--verbose', action='store_true',
                               help='verbose output')
    parser_status.add_argument('configs', nargs='*',
                               help='configs to print status for')

    parser_restart = subparsers.add_parser('restart',
                                           help='restart daemon if no configs given, or plugins in given config')
    parser_restart.set_defaults(func=restart)
    parser_restart.add_argument('--with-targets', action='store_true',
                                help='also restart the drbd-service@.target for promoter plugins')
    parser_restart.add_argument('configs', nargs='*',
                                help='configs to restart')

    parser_edit = subparsers.add_parser('edit', help='edit (or create) given config file')
    parser_edit.set_defaults(func=edit)
    parser_edit.add_argument('-t', '--type', help='plugin type',
                             choices=('promoter', 'prometheus', 'umh', 'debugger'), default='promoter')
    parser_edit.add_argument('configs', nargs=1, help='config to edit')

    parser_remove = subparsers.add_parser('rm', help='remove given config files')
    parser_remove.set_defaults(func=remove)
    parser_remove.add_argument('-f', '--force', action='store_true', help='force')
    parser_remove.add_argument('--disabled', action='store_true',
                               help='remove a disabled file.')
    parser_remove.add_argument('configs', nargs='+', help='configs to remove')

    def positive_int(arg):
        arg = int(arg)
        if arg <= 0:
            raise ValueError('must be positive')
        return arg

    parser_evict = subparsers.add_parser('evict', help='Evict promoter resource by given config files')
    parser_evict.set_defaults(func=evict)
    default_evict_delay = 20
    evict_help = 'Positive number of seconds to wait for peer takeover (default {})'.format(default_evict_delay)
    parser_evict.add_argument('-d', '--delay', type=positive_int, default=default_evict_delay,
                              help=evict_help)
    parser_evict.add_argument('-f', '--force', action='store_true',
                              help='Override checks (multiple plugins per snippet/multiple resources per promoter)')
    parser_evict.add_argument('configs', nargs='*', help='configs to evict')

    parser_cat = subparsers.add_parser('cat', help='cat given plugin config files')
    parser_cat.set_defaults(func=cat)
    parser_cat.add_argument('configs', nargs='*', help='configs to cat')

    parser_ls = subparsers.add_parser('ls', help='list enabled/disabled files and their plugins')
    parser_ls.set_defaults(func=ls)
    parser_ls.add_argument('--disabled', action='store_true', help='show disabled plugins')
    parser_ls.add_argument('configs', nargs='*', help='configs to list')

    args = parser.parse_args()

    if not os.path.isfile(args.config):
        die("main config file ('{}') does not exist".format(args.config))
    config = toml.load(args.config)
    if not config.get('snippets'):
        print('Your config ("{}") does not contain a "snippets" entry'.format(args.config))
        snippets_entry = 'snippets = "{}"'.format(DEFAULT_SNIPPETS)
        if not ask("Add a '{}' entry to '{}'?".format(snippets_entry, args.config), default=True):
            die('This tool needs a valid snippetes entry in the main config file')
        with open(args.config, 'a') as f:
            f.write('{}\n'.format(snippets_entry))
        config = toml.load(args.config)
    os.makedirs(config['snippets'], mode=0o700, exist_ok=True)

    # convenience to also use name.toml or name.disabled.toml
    for i, cfg in enumerate(args.configs):
        if cfg.endswith('.disabled'):
            cfg = cfg[:-len('.disabled')]
        if cfg.endswith('.toml'):
            cfg = cfg[:-len('.toml')]
        args.configs[i] = cfg

    if args.color != 'auto':
        global has_colors

        def has_colors(stream):
            return args.color == 'always'

    args.func(args)


if __name__ == "__main__":
    main()
