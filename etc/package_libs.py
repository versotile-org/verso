import os
import os.path as path
import shutil
import subprocess
import sys


def otool(s):
    o = subprocess.Popen(['/usr/bin/otool', '-L', s], stdout=subprocess.PIPE)
    for line in map(lambda s: s.decode('ascii'), o.stdout):
        if line[0] == '\t':
            yield line.split(' ', 1)[0][1:]


def install_name_tool(binary, *args):
    try:
        subprocess.check_call(['install_name_tool', *args, binary])
    except subprocess.CalledProcessError as e:
        print("install_name_tool exited with return value %d" % e.returncode)


def change_link_name(binary, old, new):
    install_name_tool(binary, '-change', old, f"@executable_path/{new}")


def is_system_library(lib):
    return lib.startswith("/System/Library") or lib.startswith("/usr/lib") or ".asan." in lib


def is_relocatable_library(lib):
    return lib.startswith("@rpath/")


def change_non_system_libraries_path(libraries, relative_path, binary):
    for lib in libraries:
        if is_system_library(lib) or is_relocatable_library(lib):
            continue
        new_path = path.join(relative_path, path.basename(lib))
        change_link_name(binary, lib, new_path)


def resolve_rpath(lib, rpath_root):
    if not is_relocatable_library(lib):
        return lib

    rpaths = ['', '../', 'gstreamer-1.0/']
    for rpath in rpaths:
        full_path = rpath_root + lib.replace('@rpath/', rpath)
        if path.exists(full_path):
            return path.normpath(full_path)

    raise Exception("Unable to satisfy rpath dependency: " + lib)


def copy_dependencies(binary_path, lib_path, gst_lib_dir):
    relative_path = path.relpath(lib_path, path.dirname(binary_path)) + "/"

    # Update binary libraries
    binary_dependencies = set(otool(binary_path))
    change_non_system_libraries_path(binary_dependencies, relative_path, binary_path)

    # Update dependencies libraries
    need_checked = binary_dependencies
    checked = set()
    while need_checked:
        checking = set(need_checked)
        need_checked = set()
        for f in checking:
            # No need to check these for their dylibs
            if is_system_library(f):
                continue
            full_path = resolve_rpath(f, gst_lib_dir)
            need_relinked = set(otool(full_path))
            new_path = path.join(lib_path, path.basename(full_path))
            if not path.exists(new_path):
                shutil.copyfile(full_path, new_path)
            change_non_system_libraries_path(need_relinked, relative_path, new_path)
            need_checked.update(need_relinked)
        checked.update(checking)
        need_checked.difference_update(checked)


def package_gstreamer_dylibs(bin):
    gst_root = "/Library/Frameworks/GStreamer.framework/Versions/1.0"

    # This might be None if we are cross-compiling.
    if not gst_root:
        return True

    lib_dir = path.join(path.dirname(bin), "lib")
    if os.path.exists(lib_dir):
        shutil.rmtree(lib_dir)
    os.mkdir(lib_dir)
    try:
        copy_dependencies(bin, lib_dir, path.join(gst_root, 'lib', ''))
    except Exception as e:
        print("ERROR: could not package required dylibs")
        print(e)
        return False
    return True



if __name__ == '__main__':
    try:
        subprocess.check_call(['cargo', 'build', '--release', '--features', 'packager'])
    except subprocess.CalledProcessError as e:
        print("cargo build exited with return value %d" % e.returncode)
    
    if sys.platform == "darwin":
        binary = "./target/release/verso"
        package_gstreamer_dylibs(binary)
        install_name_tool(binary, '-add_rpath', "@executable_path/../Resources/lib")
