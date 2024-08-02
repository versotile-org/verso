import os
import os.path as path
import random
import shutil
import stat
import subprocess
import sys
import time

from typing import List

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

def remove_readonly(func, path, _):
    "Clear the readonly bit and reattempt the removal"
    os.chmod(path, stat.S_IWRITE)
    func(path)

def delete(path):
    if os.path.isdir(path) and not os.path.islink(path):
        shutil.rmtree(path, onerror=remove_readonly)
    else:
        os.remove(path)

def check_call_with_randomized_backoff(args: List[str], retries: int) -> int:
    """
    Run the given command-line arguments via `subprocess.check_call()`. If the command
    fails sleep for a random number of seconds between 2 and 5 and then try to the command
    again, the given number of times.
    """
    try:
        return subprocess.check_call(args)
    except subprocess.CalledProcessError as e:
        if retries == 0:
            raise e

        sleep_time = random.uniform(2, 5)
        print(f"Running {args} failed with {e.returncode}. Trying again in {sleep_time}s")
        time.sleep(sleep_time)
        return check_call_with_randomized_backoff(args, retries - 1)

def package(binary_path):
    dir_to_root = "./"
    target_dir = path.dirname(binary_path)

    print("Creating verso.app")
    dir_to_dmg = path.join(target_dir, 'dmg')
    dir_to_app = path.join(dir_to_dmg, 'verso.app')
    dir_to_resources = path.join(dir_to_app, 'Contents', 'Resources')
    if path.exists(dir_to_dmg):
        print("Cleaning up from previous packaging")
        delete(dir_to_dmg)

    print("Copying files")
    shutil.copytree(path.join(dir_to_root, 'resources'), dir_to_resources)
    shutil.copy2(path.join(dir_to_root, 'Info.plist'), path.join(dir_to_app, 'Contents', 'Info.plist'))

    content_dir = path.join(dir_to_app, 'Contents', 'MacOS')
    lib_dir = path.join(content_dir, 'lib')
    os.makedirs(lib_dir)
    install_name_tool(binary_path, '-add_rpath', "@executable_path/lib/")
    shutil.copy2(binary_path, content_dir)

    print("Finding dylibs and relinking")
    dmg_binary = path.join(content_dir, "verso")
    gst_root = "/Library/Frameworks/GStreamer.framework/Versions/1.0"
    dir_to_gst_lib = path.join(gst_root, 'lib', '')
    copy_dependencies(dmg_binary, lib_dir, dir_to_gst_lib)

    print("Creating dmg")
    os.symlink('/Applications', path.join(dir_to_dmg, 'Applications'))
    dmg_path = path.join(target_dir, "verso_0.0.1_aarch64.dmg")

    if path.exists(dmg_path):
        print("Deleting existing dmg")
        os.remove(dmg_path)

    # `hdiutil` gives "Resource busy" failures on GitHub Actions at times. This
    # is an attempt to get around those issues by retrying the command a few times
    # after a random wait.
    try:
        check_call_with_randomized_backoff(
            ['hdiutil', 'create', '-volname', 'verso',
             '-megabytes', '900', dmg_path,
             '-srcfolder', dir_to_dmg],
            retries=3)
    except subprocess.CalledProcessError as e:
        print("Packaging MacOS dmg exited with return value %d" % e.returncode)
        return e.returncode

    print("Cleaning up")
    delete(dir_to_dmg)
    print("Packaged Verso into " + dmg_path)

if __name__ == '__main__':
    try:
        subprocess.check_call(['cargo', 'build', '--release', '--features', 'packager'])
    except subprocess.CalledProcessError as e:
        print("cargo build exited with return value %d" % e.returncode)
    
    if sys.platform == "darwin":
        binary = "./target/release/verso"
        package_gstreamer_dylibs(binary)
