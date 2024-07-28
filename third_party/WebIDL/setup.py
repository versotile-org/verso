try:
    from setuptools import setup
except ImportError:
    from distutils.core import setup

setup(name = "WebIDL",
            description="A WebIDL parser written in Python to be used in Mozilla.",
            version = "1.0.0",
            packages = ['.'],
            classifiers = [
              'Programming Language :: Python :: 3',
              ]
            )
