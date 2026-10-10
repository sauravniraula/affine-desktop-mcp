"""Fail-closed release gate; requires Python 3.11+ and authenticated gh."""

import json
import os
from pathlib import Path
import subprocess
import tomllib


def check_release(tag, manifest=Path("Cargo.toml")):
    with manifest.open("rb") as source:
        version = tomllib.load(source)["package"]["version"]
    if tag not in (version, f"v{version}"):
        return version, False, f"Tag {tag!r} does not match Cargo.toml version {version!r}."

    # List every page, including drafts visible to GITHUB_TOKEN. A failed API
    # request or malformed response must fail the job, not enable a release.
    response = subprocess.run(
        [
            "gh", "api", "--paginate", "--slurp",
            f"repos/{os.environ['GITHUB_REPOSITORY']}/releases?per_page=100",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    for page in json.loads(response.stdout):
        for release in page:
            if release["tag_name"] in (version, f"v{version}"):
                return version, False, f"Version {version} already has a release."
    return version, True, f"Version {version} is eligible for release."


def main():
    version, should_release, reason = check_release(os.environ["GITHUB_REF_NAME"])
    print(reason)
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"version={version}\n")
        output.write(f"should_release={str(should_release).lower()}\n")
        output.write(f"prerelease={str('-' in version.split('+', 1)[0]).lower()}\n")
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
        summary.write(f"{reason}\n")


if __name__ == "__main__":
    main()
