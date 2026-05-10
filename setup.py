from setuptools import find_packages, setup


setup(
    name="flight-tracker",
    version="0.1.0",
    description="CLI that finds the nearest aircraft to your current location.",
    packages=find_packages(where="src"),
    package_dir={"": "src"},
    python_requires=">=3.10",
    entry_points={
        "console_scripts": [
            "flight-tracker=flight_tracker.cli:main",
        ],
    },
)
