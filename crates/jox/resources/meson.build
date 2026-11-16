pkgdatadir = join_paths(get_option('prefix'), get_option('datadir'), meson.project_name())
gnome = import('gnome')

gnome.compile_resources(
    'jox',
    'resources.gresource.xml',
    gresource_bundle: true,
    install: true,
    install_dir: pkgdatadir,
)

desktop_file = i18n.merge_file(
    input: 'com.perwko.jox.desktop.in',
    output: 'com.perwko.jox.desktop',
    type: 'desktop',
    po_dir: '../po',
    install: true,
    install_dir: join_paths(get_option('datadir'), 'applications'),
)

desktop_utils = find_program('desktop-file-validate', required: false)
if desktop_utils.found()
    test('Validate desktop file', desktop_utils, args: [desktop_file])
endif

appstream_file = i18n.merge_file(
    input: 'com.perwko.jox.metainfo.xml.in',
    output: 'com.perwko.jox.metainfo.xml',
    po_dir: '../po',
    install: true,
    install_dir: join_paths(get_option('datadir'), 'metainfo'),
)

appstreamcli = find_program('appstreamcli', required: false, disabler: true)
test(
    'Validate appstream file',
    appstreamcli,
    args: ['validate', '--no-net', '--explain', appstream_file],
)

install_data(
    'com.perwko.jox.gschema.xml',
    install_dir: join_paths(get_option('datadir'), 'glib-2.0/schemas'),
)

compile_schemas = find_program('glib-compile-schemas', required: false)
if compile_schemas.found()
    test(
        'Validate schema file',
        compile_schemas,
        args: ['--strict', '--dry-run', meson.current_source_dir()],
    )
endif

install_subdir(
    'fonts',
    install_dir: pkgdatadir,
)

subdir('icons')