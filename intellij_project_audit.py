#!/usr/bin/env python3
"""
IntelliJ IDEA Project Audit

Read-only analyzer for an IntelliJ IDEA project/repository. It inspects:
- Repository/build-system structure
- IntelliJ .idea / .run configuration
- Git tracking and .gitignore policy
- JDK / language-level signals
- Maven / Gradle wrapper and build configuration
- Source, test, resource, and generated-source conventions
- Shared code style / inspections / run configurations
- Common machine-local IntelliJ files accidentally committed
- Portability and CI reproducibility issues

Usage:
    python intellij_project_audit.py .
    python intellij_project_audit.py /path/to/project --report idea-audit.md
    python intellij_project_audit.py . --json idea-audit.json

Exit codes:
    0 = no errors
    1 = one or more ERROR findings
    2 = invalid invocation / project path

The tool does not modify the project.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import textwrap
import xml.etree.ElementTree as ET
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable, Optional


# -----------------------------
# Data model
# -----------------------------

SEVERITY_ORDER = {"ERROR": 0, "WARN": 1, "INFO": 2, "PASS": 3}


@dataclass
class Finding:
    severity: str
    category: str
    title: str
    detail: str
    recommendation: str = ""
    paths: tuple[str, ...] = ()


@dataclass
class ProjectFacts:
    root: str
    git_repo: bool
    build_systems: list[str]
    languages: list[str]
    has_idea: bool
    has_run_dir: bool
    tracked_file_count: Optional[int]
    java_version_signals: list[str]
    modules: list[str]


# -----------------------------
# Helpers
# -----------------------------

def rel(path: Path, root: Path) -> str:
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except Exception:
        return str(path)


def read_text(path: Path, limit: int = 2_000_000) -> str:
    try:
        if path.stat().st_size > limit:
            return ""
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def safe_xml_parse(path: Path) -> Optional[ET.Element]:
    try:
        return ET.parse(path).getroot()
    except (ET.ParseError, OSError):
        return None


def run_git(root: Path, *args: str) -> tuple[int, str]:
    try:
        p = subprocess.run(
            ["git", "-C", str(root), *args],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=10,
            check=False,
        )
        return p.returncode, p.stdout
    except (OSError, subprocess.TimeoutExpired):
        return 127, ""


def git_tracked(root: Path) -> Optional[set[str]]:
    if not shutil.which("git"):
        return None
    code, out = run_git(root, "rev-parse", "--is-inside-work-tree")
    if code != 0 or out.strip() != "true":
        return None
    code, out = run_git(root, "ls-files", "-z")
    if code != 0:
        return None
    return {p for p in out.split("\0") if p}


def git_ignored(root: Path, path: str) -> Optional[bool]:
    if not shutil.which("git"):
        return None
    try:
        p = subprocess.run(
            ["git", "-C", str(root), "check-ignore", "-q", "--", path],
            timeout=5,
            check=False,
        )
        return p.returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return None


def add(
    findings: list[Finding],
    severity: str,
    category: str,
    title: str,
    detail: str,
    recommendation: str = "",
    paths: Iterable[str] = (),
) -> None:
    findings.append(
        Finding(
            severity=severity,
            category=category,
            title=title,
            detail=detail.strip(),
            recommendation=recommendation.strip(),
            paths=tuple(paths),
        )
    )


def has_any(root: Path, names: Iterable[str]) -> bool:
    return any((root / n).exists() for n in names)


def find_files(root: Path, patterns: Iterable[str], max_results: int = 5000) -> list[Path]:
    out: list[Path] = []
    skip_dirs = {
        ".git", ".gradle", ".mvn/wrapper", "node_modules", "target", "build",
        "out", ".venv", "venv", "__pycache__", ".pytest_cache", ".tox",
    }
    for pattern in patterns:
        for p in root.rglob(pattern):
            try:
                rp = rel(p, root)
            except Exception:
                rp = str(p)
            if any(part in skip_dirs for part in p.parts):
                continue
            if "/.git/" in f"/{rp}/":
                continue
            out.append(p)
            if len(out) >= max_results:
                return out
    return out


# -----------------------------
# Detection
# -----------------------------

def detect_build_systems(root: Path) -> list[str]:
    systems = []
    if has_any(root, ["pom.xml"]):
        systems.append("Maven")
    if has_any(root, ["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"]):
        systems.append("Gradle")
    if has_any(root, ["build.sbt"]):
        systems.append("sbt")
    if has_any(root, ["WORKSPACE", "WORKSPACE.bazel", "MODULE.bazel", "BUILD", "BUILD.bazel"]):
        systems.append("Bazel")
    if has_any(root, ["package.json"]):
        systems.append("Node/npm")
    if has_any(root, ["pyproject.toml", "requirements.txt", "setup.py", "Pipfile"]):
        systems.append("Python")
    if has_any(root, ["Cargo.toml"]):
        systems.append("Cargo")
    return systems or ["IDE/manual/unknown"]


def detect_languages(root: Path) -> list[str]:
    suffixes = {
        ".java": "Java",
        ".kt": "Kotlin",
        ".kts": "Kotlin",
        ".groovy": "Groovy",
        ".scala": "Scala",
        ".py": "Python",
        ".js": "JavaScript",
        ".ts": "TypeScript",
        ".rs": "Rust",
    }
    found = set()
    count = 0
    for base, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in {
            ".git", ".gradle", "node_modules", "target", "build", "out",
            ".venv", "venv", "__pycache__"
        }]
        for name in files:
            ext = Path(name).suffix.lower()
            if ext in suffixes:
                found.add(suffixes[ext])
            count += 1
            if count > 20_000:
                return sorted(found)
    return sorted(found)


def detect_modules(root: Path) -> list[str]:
    modules = []
    for name in ("settings.gradle", "settings.gradle.kts"):
        p = root / name
        if p.exists():
            text = read_text(p)
            # Common forms: include("a", "b"), include ':a', ':b'
            for m in re.findall(r'include\s*\((.*?)\)', text, flags=re.S):
                modules.extend(re.findall(r'["\']([^"\']+)["\']', m))
            for line in text.splitlines():
                if re.match(r"\s*include\s+['\"]", line):
                    modules.extend(re.findall(r'["\']:?([^"\']+)["\']', line))
    if (root / "pom.xml").exists():
        xml = safe_xml_parse(root / "pom.xml")
        if xml is not None:
            for elem in xml.iter():
                if elem.tag.endswith("module") and elem.text:
                    modules.append(elem.text.strip())
    return sorted({m for m in modules if m})


def detect_java_version_signals(root: Path) -> list[str]:
    signals: list[str] = []

    # Maven
    pom = root / "pom.xml"
    if pom.exists():
        text = read_text(pom)
        patterns = [
            (r"<maven\.compiler\.release>\s*([^<]+)", "Maven compiler release"),
            (r"<maven\.compiler\.source>\s*([^<]+)", "Maven compiler source"),
            (r"<maven\.compiler\.target>\s*([^<]+)", "Maven compiler target"),
            (r"<release>\s*([^<]+)", "Maven plugin release"),
            (r"<source>\s*([^<]+)", "Maven plugin source"),
            (r"<target>\s*([^<]+)", "Maven plugin target"),
        ]
        for pattern, label in patterns:
            for value in re.findall(pattern, text, flags=re.I):
                value = value.strip()
                if value and not value.startswith("${"):
                    signals.append(f"{label}: {value}")

    # Gradle
    for name in ("build.gradle", "build.gradle.kts"):
        p = root / name
        if p.exists():
            text = read_text(p)
            patterns = [
                (r"JavaLanguageVersion\.of\((\d+)\)", "Gradle Java toolchain"),
                (r"languageVersion\s*=\s*JavaLanguageVersion\.of\((\d+)\)", "Gradle Java toolchain"),
                (r"sourceCompatibility\s*=\s*['\"]?([A-Za-z0-9_.]+)", "Gradle sourceCompatibility"),
                (r"targetCompatibility\s*=\s*['\"]?([A-Za-z0-9_.]+)", "Gradle targetCompatibility"),
                (r"jvmToolchain\((\d+)\)", "Kotlin JVM toolchain"),
            ]
            for pattern, label in patterns:
                for value in re.findall(pattern, text):
                    signals.append(f"{label}: {value}")

    # .java-version / SDKMAN / Gradle JVM properties
    for name in (".java-version", ".sdkmanrc"):
        p = root / name
        if p.exists():
            content = read_text(p).strip()
            if content:
                signals.append(f"{name}: {content[:120]}")

    gradle_props = root / "gradle.properties"
    if gradle_props.exists():
        text = read_text(gradle_props)
        for line in text.splitlines():
            if line.strip().startswith("org.gradle.java.home="):
                signals.append("gradle.properties sets org.gradle.java.home")

    return sorted(set(signals))


# -----------------------------
# Audits
# -----------------------------

def audit_git(root: Path, findings: list[Finding], tracked: Optional[set[str]]) -> None:
    category = "Git & portability"
    gitignore = root / ".gitignore"

    if tracked is None:
        add(
            findings, "INFO", category, "Git tracking could not be inspected",
            "The project is not a Git worktree, Git is unavailable, or tracked files could not be read.",
            "Run this audit inside the repository root with Git installed for commit-policy checks."
        )
        return

    add(
        findings, "PASS", category, "Git repository detected",
        f"{len(tracked)} tracked files were inspected."
    )

    if not gitignore.exists():
        add(
            findings, "WARN", category, "No root .gitignore found",
            "IDE-local, build-output, and machine-specific files are easier to commit accidentally.",
            "Add a project .gitignore covering build outputs and IntelliJ machine-local state.",
            [".gitignore"],
        )

    machine_local = [
        ".idea/workspace.xml",
        ".idea/tasks.xml",
        ".idea/usage.statistics.xml",
        ".idea/shelf",
        ".idea/httpRequests",
        ".idea/dataSources.local.xml",
        ".idea/sonarlint",
    ]
    bad_tracked = []
    for item in machine_local:
        if item in tracked or any(p.startswith(item.rstrip("/") + "/") for p in tracked):
            bad_tracked.append(item)

    if bad_tracked:
        add(
            findings, "ERROR", category, "Machine-local IntelliJ state is committed",
            "These files/directories commonly contain per-user state and create noisy diffs or portability problems.",
            "Remove them from Git and add appropriate ignore rules.",
            bad_tracked,
        )
    else:
        add(
            findings, "PASS", category, "No common machine-local IntelliJ state is tracked",
            "Common per-user IntelliJ files were not found in Git."
        )

    imls = sorted(p for p in tracked if p.endswith(".iml"))
    if imls:
        add(
            findings, "WARN", category, "IntelliJ .iml module files are tracked",
            "For Maven/Gradle projects, module metadata is usually better regenerated from the build model. "
            "Tracking .iml can cause IDE-version and machine-specific churn.",
            "If this is a Maven/Gradle project, prefer ignoring *.iml and import modules from the build tool. "
            "Keep them only when the IDE project model itself is authoritative.",
            imls[:20],
        )

    build_outputs = [
        p for p in tracked
        if re.search(r"(^|/)(target|build|out|\.gradle|node_modules)/", p)
    ]
    if build_outputs:
        add(
            findings, "ERROR", category, "Generated/build output is tracked",
            f"{len(build_outputs)} tracked files appear under build-output directories.",
            "Remove generated output from version control and ignore the relevant directories.",
            build_outputs[:20],
        )


def audit_idea(root: Path, findings: list[Finding], tracked: Optional[set[str]]) -> None:
    category = "IntelliJ IDEA"
    idea = root / ".idea"
    run_dir = root / ".run"

    if not idea.exists():
        add(
            findings, "INFO", category, "No .idea directory found",
            "This is fine for projects imported entirely from Maven/Gradle/Bazel, but it means no shared IntelliJ project settings are present.",
            "Optionally share stable team settings such as code style, inspections, or run configurations when they add value."
        )
        return

    add(findings, "PASS", category, ".idea directory detected", "IntelliJ project metadata is present.", paths=[".idea"])

    # Useful shared project settings
    shareable_candidates = {
        ".idea/codeStyles": "project code style",
        ".idea/inspectionProfiles": "inspection profile",
        ".idea/runConfigurations": "legacy shared run configurations",
        ".run": "shared run configurations",
        ".idea/dictionaries": "project dictionary",
        ".idea/scopes": "project scopes",
    }

    present = []
    for p, label in shareable_candidates.items():
        if (root / p).exists():
            present.append(f"{p} ({label})")
    if present:
        add(
            findings, "PASS", category, "Reusable team IDE configuration found",
            "Detected: " + ", ".join(present)
        )
    else:
        add(
            findings, "INFO", category, "No shared code-style/inspection/run configuration detected",
            "The repository does not appear to contain common shareable IntelliJ team settings.",
            "If the team relies on IDE formatting, inspections, or complex run/debug setups, consider sharing stable project-level settings."
        )

    # Check whether present team config is actually tracked
    if tracked is not None:
        useful_paths = []
        for p in (".idea/codeStyles", ".idea/inspectionProfiles", ".idea/runConfigurations", ".run"):
            if (root / p).exists():
                if any(x == p or x.startswith(p.rstrip("/") + "/") for x in tracked):
                    useful_paths.append(p)
        if present and not useful_paths:
            add(
                findings, "WARN", category, "IDE team settings exist but are not tracked",
                "Shareable project settings appear on disk but not in Git, so other developers and CI helpers will not receive them.",
                "Commit only stable, team-relevant IntelliJ project settings; keep per-user state ignored."
            )

    # workspace.xml
    ws = idea / "workspace.xml"
    if ws.exists() and tracked is not None and ".idea/workspace.xml" in tracked:
        add(
            findings, "ERROR", category, "workspace.xml is tracked",
            "workspace.xml stores user/session-specific IntelliJ state and commonly creates noisy conflicts.",
            "Remove .idea/workspace.xml from Git and ignore it.",
            [".idea/workspace.xml"],
        )

    # VCS mapping
    vcs = idea / "vcs.xml"
    if vcs.exists():
        text = read_text(vcs)
        if 'vcs="Git"' in text or "vcs='Git'" in text:
            add(findings, "PASS", category, "Git VCS mapping configured", ".idea/vcs.xml maps the project to Git.", paths=[".idea/vcs.xml"])
        else:
            add(findings, "INFO", category, "VCS mapping present", "A .idea/vcs.xml file exists; verify its mapping matches the repository.", paths=[".idea/vcs.xml"])

    # Hard-coded paths in XML
    suspicious = []
    for p in idea.rglob("*.xml"):
        text = read_text(p)
        if not text:
            continue
        if re.search(r"(file://)?/(Users|home)/[^/$<]+/", text) or re.search(r"[A-Za-z]:\\Users\\[^\\]+\\", text):
            suspicious.append(rel(p, root))
    if suspicious:
        add(
            findings, "WARN", category, "Possible absolute user paths in IntelliJ metadata",
            "Absolute home-directory paths can make shared IDE configuration non-portable.",
            "Replace machine-specific paths with $PROJECT_DIR$, $MODULE_DIR$, environment variables, or build-tool configuration where possible.",
            suspicious[:20],
        )


def audit_gradle(root: Path, findings: list[Finding]) -> None:
    if not has_any(root, ["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"]):
        return
    category = "Gradle"

    wrapper_jar = root / "gradle/wrapper/gradle-wrapper.jar"
    wrapper_props = root / "gradle/wrapper/gradle-wrapper.properties"
    unix = root / "gradlew"
    win = root / "gradlew.bat"

    missing = [rel(p, root) for p in (wrapper_jar, wrapper_props, unix, win) if not p.exists()]
    if missing:
        add(
            findings, "WARN", category, "Gradle Wrapper is incomplete",
            "A complete Gradle Wrapper makes local development and CI use the same Gradle version.",
            "Generate/commit the wrapper scripts, wrapper properties, and wrapper JAR.",
            missing,
        )
    else:
        add(findings, "PASS", category, "Gradle Wrapper present", "Wrapper scripts and wrapper metadata are present.")

    settings = root / "settings.gradle.kts"
    if not settings.exists():
        settings = root / "settings.gradle"
    if settings.exists():
        add(findings, "PASS", category, "Gradle settings file present", "The project has an explicit Gradle settings file.", paths=[rel(settings, root)])
    else:
        add(
            findings, "WARN", category, "No Gradle settings file found",
            "Single-project builds can work without one, but an explicit settings file improves project naming and multi-module structure.",
            "Add settings.gradle(.kts) with a stable rootProject.name, especially for team projects."
        )

    texts = "\n".join(
        read_text(p) for p in [root / "build.gradle", root / "build.gradle.kts"] if p.exists()
    )
    if "toolchain" in texts or "JavaLanguageVersion.of" in texts or "jvmToolchain" in texts:
        add(findings, "PASS", category, "JVM toolchain configuration detected", "The build appears to define a Java/Kotlin toolchain, which improves IDE/CI reproducibility.")
    elif any((root / n).exists() for n in (".java-version", ".sdkmanrc")):
        add(findings, "INFO", category, "External JDK version hint found", "A JDK version hint exists outside Gradle, but the build itself may not enforce the toolchain.")
    else:
        add(
            findings, "WARN", category, "No Gradle JVM toolchain detected",
            "Developers and CI may compile with different JDKs if the environment supplies different defaults.",
            "Prefer Gradle Java toolchains (and Kotlin jvmToolchain where applicable) for reproducible JDK selection."
        )

    gp = root / "gradle.properties"
    if gp.exists():
        text = read_text(gp)
        m = re.search(r"(?m)^\s*org\.gradle\.java\.home\s*=\s*(.+)$", text)
        if m:
            value = m.group(1).strip()
            if value.startswith("/") or re.match(r"^[A-Za-z]:[\\/]", value):
                add(
                    findings, "ERROR", category, "Absolute org.gradle.java.home detected",
                    f"gradle.properties pins Gradle to a machine-specific JDK path: {value}",
                    "Remove machine-specific org.gradle.java.home from the shared project file; use toolchains or user-level Gradle properties instead.",
                    ["gradle.properties"],
                )


def audit_maven(root: Path, findings: list[Finding]) -> None:
    pom = root / "pom.xml"
    if not pom.exists():
        return
    category = "Maven"
    root_xml = safe_xml_parse(pom)

    if root_xml is None:
        add(findings, "ERROR", category, "pom.xml could not be parsed", "The root Maven POM is not valid XML or could not be read.", paths=["pom.xml"])
        return

    add(findings, "PASS", category, "Maven POM parsed", "The root pom.xml is valid XML.", paths=["pom.xml"])

    wrapper = [root / "mvnw", root / "mvnw.cmd", root / ".mvn/wrapper/maven-wrapper.properties"]
    if all(p.exists() for p in wrapper):
        add(findings, "PASS", category, "Maven Wrapper present", "Wrapper scripts and wrapper properties are present.")
    else:
        add(
            findings, "INFO", category, "Maven Wrapper not fully present",
            "A Maven Wrapper is optional but improves consistency between developers and CI.",
            "Consider committing Maven Wrapper files if reproducible Maven versions are important.",
            [rel(p, root) for p in wrapper if not p.exists()],
        )

    text = read_text(pom)
    if re.search(r"<maven\.compiler\.release>", text) or re.search(r"<release>", text):
        add(findings, "PASS", category, "Java release level configured", "Maven compiler release configuration was detected.")
    elif re.search(r"<maven\.compiler\.(source|target)>", text):
        add(
            findings, "INFO", category, "Maven source/target configured",
            "Java source/target levels are set. For modern JDKs, --release is often a stronger compatibility signal.",
            "Consider maven.compiler.release when appropriate for the project."
        )
    else:
        add(
            findings, "WARN", category, "No explicit Maven compiler level detected",
            "IDE and CI may infer different Java language/bytecode levels depending on plugin/JDK defaults.",
            "Set an explicit compiler release/source/target compatible with the project."
        )


def audit_sources(root: Path, findings: list[Finding], build_systems: list[str], languages: list[str]) -> None:
    category = "Project structure"

    conventional = {
        "src/main/java": "main Java sources",
        "src/main/kotlin": "main Kotlin sources",
        "src/main/resources": "main resources",
        "src/test/java": "Java tests",
        "src/test/kotlin": "Kotlin tests",
        "src/test/resources": "test resources",
    }
    present = [p for p in conventional if (root / p).exists()]
    if present:
        add(findings, "PASS", category, "Conventional source roots detected", ", ".join(present))

    javaish = bool({"Java", "Kotlin", "Groovy", "Scala"} & set(languages))
    if javaish and ("Maven" in build_systems or "Gradle" in build_systems) and not present:
        add(
            findings, "INFO", category, "No standard JVM source roots at repository root",
            "This may be a multi-module project, or the build may define custom source sets.",
            "Ensure source/test/resource roots are declared in the build tool rather than only marked manually in IntelliJ."
        )

    # Generated source directories that might need build-tool ownership
    generated_dirs = []
    for candidate in (
        "src/generated", "src/main/generated", "generated", "generated-src",
        "build/generated", "target/generated-sources",
    ):
        if (root / candidate).exists():
            generated_dirs.append(candidate)

    if generated_dirs:
        add(
            findings, "INFO", category, "Generated-source directories detected",
            "Generated code should normally be produced and registered by the build system so IntelliJ imports it consistently.",
            "Verify generated-source roots come from Maven/Gradle configuration and generated output is not committed unless intentionally vendored.",
            generated_dirs,
        )

    # Nested modules
    poms = [p for p in root.rglob("pom.xml") if ".git" not in p.parts and "target" not in p.parts]
    gradle_builds = [
        p for pat in ("build.gradle", "build.gradle.kts")
        for p in root.rglob(pat)
        if ".git" not in p.parts and "build" not in p.parts
    ]
    count = max(len(poms), len(gradle_builds))
    if count > 1:
        add(findings, "PASS", category, "Multi-module layout detected", f"Detected {count} module build files.")


def audit_editorconfig(root: Path, findings: list[Finding]) -> None:
    category = "Code style"
    editor = root / ".editorconfig"
    idea_styles = root / ".idea/codeStyles"

    if editor.exists():
        add(
            findings, "PASS", category, ".editorconfig present",
            "EditorConfig provides IDE-independent formatting basics and IntelliJ IDEA supports it.",
            paths=[".editorconfig"],
        )
    elif idea_styles.exists():
        add(
            findings, "INFO", category, "IntelliJ code style exists without .editorconfig",
            "Formatting may be consistent inside IntelliJ but less portable to other editors/tools.",
            "Consider adding .editorconfig for baseline cross-editor formatting while keeping IntelliJ-specific rules where needed."
        )
    else:
        add(
            findings, "INFO", category, "No shared formatting policy detected",
            "Neither .editorconfig nor .idea/codeStyles was found.",
            "For team projects, share formatting policy through .editorconfig and/or IntelliJ project code style."
        )

    inspections = root / ".idea/inspectionProfiles"
    if inspections.exists():
        add(findings, "PASS", category, "Shared IntelliJ inspection profile detected", "Project inspection settings are available to the team.", paths=[".idea/inspectionProfiles"])
    else:
        add(
            findings, "INFO", category, "No shared IntelliJ inspection profile detected",
            "Developers may see different IDE diagnostics depending on personal settings.",
            "If consistent IDE inspections matter, save a project inspection profile and commit only the stable project-level files."
        )


def audit_run_configs(root: Path, findings: list[Finding], tracked: Optional[set[str]]) -> None:
    category = "Run/debug"
    run_dir = root / ".run"
    legacy = root / ".idea/runConfigurations"

    configs = []
    for base in (run_dir, legacy):
        if base.exists():
            configs.extend(base.rglob("*.xml"))

    if not configs:
        add(
            findings, "INFO", category, "No shared run/debug configurations found",
            "This is fine for simple projects. Complex services, integration tests, or compound launches may benefit from shared configurations.",
            "Store reusable run configurations in .run/ or as shared project configurations when they reduce setup friction."
        )
        return

    add(
        findings, "PASS", category, "Shared run/debug configurations detected",
        f"Found {len(configs)} run configuration file(s).",
        paths=[rel(p, root) for p in configs[:20]],
    )

    suspicious = []
    secrets = []
    secret_re = re.compile(r"(?i)(password|passwd|token|secret|api[_-]?key)\s*[\"'=:\s]+[^<\s\"]+")
    for p in configs:
        text = read_text(p)
        if re.search(r"(file://)?/(Users|home)/[^/$<]+/", text) or re.search(r"[A-Za-z]:\\Users\\", text):
            suspicious.append(rel(p, root))
        if secret_re.search(text):
            secrets.append(rel(p, root))

    if suspicious:
        add(
            findings, "WARN", category, "Possible machine-specific paths in run configurations",
            "Shared run configurations should avoid absolute per-user paths.",
            "Use $PROJECT_DIR$, $MODULE_DIR$, environment variables, or build-tool tasks.",
            suspicious[:20],
        )

    if secrets:
        add(
            findings, "ERROR", category, "Possible secrets in run configurations",
            "Run configurations can accidentally contain credentials or API tokens.",
            "Move sensitive values to untracked environment files, a secret manager, or developer-local environment variables.",
            secrets[:20],
        )


def audit_annotation_processing(root: Path, findings: list[Finding], build_systems: list[str]) -> None:
    category = "Annotation processing"
    compiler_xml = root / ".idea/compiler.xml"
    if compiler_xml.exists():
        text = read_text(compiler_xml)
        if "annotationProcessing" in text or "AnnotationProcessing" in text:
            if "Maven" in build_systems or "Gradle" in build_systems:
                add(
                    findings, "INFO", category, "IntelliJ annotation-processing settings detected",
                    "For build-tool projects, annotation processing should ideally be defined by Maven/Gradle so IDE and CI agree.",
                    "Verify the build defines processors/dependencies and generated-source behavior, rather than relying only on .idea/compiler.xml.",
                    [".idea/compiler.xml"],
                )

    # Signals in build files
    build_text = ""
    for p in (root / "pom.xml", root / "build.gradle", root / "build.gradle.kts"):
        if p.exists():
            build_text += "\n" + read_text(p)

    if re.search(r"annotationProcessor|kapt\b|ksp\b|annotationProcessorPaths", build_text, flags=re.I):
        add(
            findings, "PASS", category, "Build-managed annotation processing detected",
            "Annotation processor configuration appears in the build, which improves IDE/CI consistency."
        )


def audit_idea_sdk(root: Path, findings: list[Finding], build_systems: list[str]) -> None:
    category = "SDK & language level"
    misc = root / ".idea/misc.xml"
    if not misc.exists():
        add(
            findings, "INFO", category, "No .idea/misc.xml SDK metadata found",
            "For build-tool projects, IntelliJ can often infer project language level from Maven/Gradle.",
        )
        return

    root_xml = safe_xml_parse(misc)
    if root_xml is None:
        add(findings, "WARN", category, ".idea/misc.xml could not be parsed", "Project SDK metadata may be malformed.", paths=[".idea/misc.xml"])
        return

    attrs = []
    for elem in root_xml.iter():
        for key in ("project-jdk-name", "project-jdk-type", "languageLevel", "default"):
            if key in elem.attrib:
                attrs.append(f"{key}={elem.attrib[key]}")
    if attrs:
        add(findings, "INFO", category, "IntelliJ SDK/language metadata detected", "; ".join(sorted(set(attrs))), paths=[".idea/misc.xml"])

    text = read_text(misc)
    if re.search(r"project-jdk-name=\"[^\"]*(/|\\\\)", text):
        add(
            findings, "WARN", category, "Project JDK name looks path-like",
            "Shared IntelliJ SDK references should not encode machine-specific installation paths.",
            "Use a stable SDK name and enforce the actual JDK via Maven/Gradle toolchains or environment provisioning.",
            [".idea/misc.xml"],
        )

    if "Maven" in build_systems or "Gradle" in build_systems:
        add(
            findings, "INFO", category, "Build tool should remain source of truth",
            "For imported Maven/Gradle projects, keep language level, dependencies, source roots, generated sources, and compiler behavior in the build model whenever possible.",
        )


def audit_ci(root: Path, findings: list[Finding], build_systems: list[str]) -> None:
    category = "CI reproducibility"
    ci_markers = [
        ".github/workflows", ".gitlab-ci.yml", "Jenkinsfile", ".circleci",
        "azure-pipelines.yml", "bitbucket-pipelines.yml",
    ]
    present = [p for p in ci_markers if (root / p).exists()]
    if present:
        add(findings, "PASS", category, "CI configuration detected", "Detected: " + ", ".join(present), paths=present)
    else:
        add(
            findings, "INFO", category, "No common CI configuration detected",
            "The repository may use an external CI setup, but no common in-repository CI config was found.",
            "For team projects, CI should validate builds/tests independently of IntelliJ."
        )

    if "Gradle" in build_systems and not (root / "gradlew").exists():
        add(
            findings, "WARN", category, "CI may depend on system Gradle",
            "No ./gradlew was found at the project root.",
            "Commit and use the Gradle Wrapper in CI."
        )
    if "Maven" in build_systems and not (root / "mvnw").exists():
        add(
            findings, "INFO", category, "CI may depend on system Maven",
            "No ./mvnw was found at the project root.",
            "Consider Maven Wrapper if build-tool version consistency matters."
        )


def audit_gitignore_policy(root: Path, findings: list[Finding]) -> None:
    category = "Git & portability"
    p = root / ".gitignore"
    if not p.exists():
        return

    text = read_text(p)
    checks = {
        "IntelliJ workspace": [r"(^|/)\.idea/workspace\.xml", r"workspace\.xml"],
        "IntelliJ shelf": [r"\.idea/shelf", r"(^|/)shelf/"],
        "IntelliJ HTTP scratch": [r"\.idea/httpRequests", r"httpRequests"],
        "IML files": [r"\*\.iml", r"\.iml$"],
        "Gradle output": [r"(^|/)\.gradle/?$", r"(^|/)build/?$"],
        "Maven output": [r"(^|/)target/?$"],
        "IDE output": [r"(^|/)out/?$"],
    }

    build_systems = detect_build_systems(root)
    expected = ["IntelliJ workspace", "IntelliJ shelf", "IntelliJ HTTP scratch", "IDE output"]
    if "Gradle" in build_systems:
        expected.append("Gradle output")
    if "Maven" in build_systems:
        expected.append("Maven output")
    if "Gradle" in build_systems or "Maven" in build_systems:
        expected.append("IML files")

    missing = []
    for label in expected:
        pats = checks[label]
        if not any(re.search(pat, text, flags=re.M) for pat in pats):
            missing.append(label)

    if missing:
        add(
            findings, "INFO", category, ".gitignore may be missing common IntelliJ/build rules",
            "Potentially uncovered categories: " + ", ".join(missing),
            "Review the suggested policy section in this report. Do not blindly ignore all of .idea if your team intentionally shares stable project settings.",
            [".gitignore"],
        )
    else:
        add(findings, "PASS", category, ".gitignore covers common IDE/build outputs", "Common IntelliJ and build-output patterns appear to be covered.", paths=[".gitignore"])


# -----------------------------
# Scoring & report
# -----------------------------

def calculate_score(findings: list[Finding]) -> int:
    score = 100
    for f in findings:
        if f.severity == "ERROR":
            score -= 14
        elif f.severity == "WARN":
            score -= 6
        # INFO does not penalize; PASS does not add.
    return max(0, min(100, score))


def grade(score: int) -> str:
    if score >= 90:
        return "Excellent"
    if score >= 80:
        return "Good"
    if score >= 65:
        return "Needs attention"
    return "High risk / inconsistent"


def recommended_gitignore(build_systems: list[str]) -> str:
    lines = [
        "# IntelliJ IDEA: keep stable team settings, ignore user/session state",
        ".idea/workspace.xml",
        ".idea/tasks.xml",
        ".idea/usage.statistics.xml",
        ".idea/shelf/",
        ".idea/httpRequests/",
        ".idea/dataSources.local.xml",
        "",
        "# IntelliJ generated module/output files",
        "*.iml",
        "out/",
    ]
    if "Gradle" in build_systems:
        lines += ["", "# Gradle", ".gradle/", "build/"]
    if "Maven" in build_systems:
        lines += ["", "# Maven", "target/"]
    lines += [
        "",
        "# OS noise",
        ".DS_Store",
        "Thumbs.db",
        "",
        "# Optional local env/secrets",
        ".env",
        ".env.local",
    ]
    return "\n".join(lines)


def render_markdown(facts: ProjectFacts, findings: list[Finding], score: int) -> str:
    counts = {s: sum(1 for f in findings if f.severity == s) for s in SEVERITY_ORDER}
    findings_sorted = sorted(findings, key=lambda f: (SEVERITY_ORDER[f.severity], f.category, f.title))

    def md_escape(s: str) -> str:
        return s.replace("|", "\\|")

    sections = []
    sections.append(f"# IntelliJ IDEA Project Audit\n")
    sections.append(
        f"**Project:** `{facts.root}`  \n"
        f"**Score:** **{score}/100 — {grade(score)}**  \n"
        f"**Findings:** {counts['ERROR']} error(s), {counts['WARN']} warning(s), "
        f"{counts['INFO']} info item(s), {counts['PASS']} pass(es)\n"
    )

    sections.append("## Project facts\n")
    sections.append(
        "| Item | Value |\n|---|---|\n"
        f"| Git repository | {'Yes' if facts.git_repo else 'No / unavailable'} |\n"
        f"| Build systems | {md_escape(', '.join(facts.build_systems))} |\n"
        f"| Languages detected | {md_escape(', '.join(facts.languages) or 'Unknown')} |\n"
        f"| `.idea/` | {'Present' if facts.has_idea else 'Absent'} |\n"
        f"| `.run/` | {'Present' if facts.has_run_dir else 'Absent'} |\n"
        f"| Tracked files | {facts.tracked_file_count if facts.tracked_file_count is not None else 'Unknown'} |\n"
        f"| Modules detected | {md_escape(', '.join(facts.modules) if facts.modules else 'None / not inferred')} |\n"
        f"| Java/JDK signals | {md_escape('; '.join(facts.java_version_signals) if facts.java_version_signals else 'None detected')} |\n"
    )

    sections.append("## Priority findings\n")
    actionable = [f for f in findings_sorted if f.severity in ("ERROR", "WARN")]
    if not actionable:
        sections.append("No error or warning findings were detected.\n")
    else:
        for f in actionable:
            sections.append(f"### [{f.severity}] {f.title}\n")
            sections.append(f"**Category:** {f.category}\n\n{f.detail}\n")
            if f.paths:
                sections.append("**Paths:** " + ", ".join(f"`{p}`" for p in f.paths) + "\n")
            if f.recommendation:
                sections.append(f"**Recommendation:** {f.recommendation}\n")

    sections.append("## All checks\n")
    sections.append("| Severity | Category | Check | Detail |\n|---|---|---|---|\n")
    for f in findings_sorted:
        detail = " ".join(f.detail.split())
        sections.append(
            f"| {f.severity} | {md_escape(f.category)} | {md_escape(f.title)} | {md_escape(detail)} |\n"
        )

    sections.append("## Recommended IntelliJ/Git policy\n")
    sections.append(textwrap.dedent("""
    A practical default for Maven/Gradle repositories is:

    - Treat the build tool as the source of truth for dependencies, modules, source roots, generated sources, compiler level, annotation processors, and test layout.
    - Share only stable IntelliJ project settings that improve team consistency, such as project code style, inspection profiles, and genuinely reusable run/debug configurations.
    - Do **not** commit user/session state such as `workspace.xml`, shelves, local HTTP requests, or local data-source state.
    - Prefer `.run/*.run.xml` or shared project run configurations for reusable launches; keep secrets in environment variables or secret-management tooling.
    - Prefer `.editorconfig` for portable baseline formatting, supplemented by IntelliJ-specific code-style settings only where useful.
    - Use Maven/Gradle wrappers and explicit JDK/toolchain configuration so IntelliJ and CI resolve the same build model.
    - For Maven/Gradle projects, normally let IntelliJ regenerate `*.iml` files instead of versioning them.
    """).strip() + "\n")

    sections.append("## Suggested `.gitignore` baseline\n")
    sections.append("```gitignore\n" + recommended_gitignore(facts.build_systems) + "\n```\n")

    sections.append("## Interpretation\n")
    sections.append(
        "This audit is heuristic. IntelliJ IDEA supports many valid project models, and some repositories intentionally "
        "version more or less IDE metadata. A finding should be judged against whether Maven/Gradle/Bazel or the IntelliJ "
        "project model is intended to be authoritative.\n"
    )
    return "\n".join(sections)


def audit(root: Path) -> tuple[ProjectFacts, list[Finding], int]:
    findings: list[Finding] = []
    tracked = git_tracked(root)
    build_systems = detect_build_systems(root)
    languages = detect_languages(root)

    audit_git(root, findings, tracked)
    audit_idea(root, findings, tracked)
    audit_gitignore_policy(root, findings)
    audit_gradle(root, findings)
    audit_maven(root, findings)
    audit_sources(root, findings, build_systems, languages)
    audit_editorconfig(root, findings)
    audit_run_configs(root, findings, tracked)
    audit_annotation_processing(root, findings, build_systems)
    audit_idea_sdk(root, findings, build_systems)
    audit_ci(root, findings, build_systems)

    facts = ProjectFacts(
        root=str(root.resolve()),
        git_repo=tracked is not None,
        build_systems=build_systems,
        languages=languages,
        has_idea=(root / ".idea").is_dir(),
        has_run_dir=(root / ".run").is_dir(),
        tracked_file_count=len(tracked) if tracked is not None else None,
        java_version_signals=detect_java_version_signals(root),
        modules=detect_modules(root),
    )
    score = calculate_score(findings)
    return facts, findings, score


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Analyze an IntelliJ IDEA project/repository and generate a configuration report."
    )
    parser.add_argument(
        "project",
        nargs="?",
        default=".",
        help="Project/repository root to inspect (default: current directory).",
    )
    parser.add_argument(
        "--report",
        default="intellij-idea-audit.md",
        help="Markdown report path. Relative paths are created inside the project root.",
    )
    parser.add_argument(
        "--json",
        dest="json_report",
        default=None,
        help="Optional JSON report path.",
    )
    parser.add_argument(
        "--no-write",
        action="store_true",
        help="Print the Markdown report to stdout instead of writing a report file.",
    )
    args = parser.parse_args()

    root = Path(args.project).expanduser()
    if not root.exists() or not root.is_dir():
        print(f"error: project path is not a directory: {root}", file=sys.stderr)
        return 2

    facts, findings, score = audit(root)
    markdown = render_markdown(facts, findings, score)

    if args.no_write:
        print(markdown)
    else:
        report_path = Path(args.report)
        if not report_path.is_absolute():
            report_path = root / report_path
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(markdown, encoding="utf-8")
        print(f"Wrote Markdown report: {report_path}")

    if args.json_report:
        json_path = Path(args.json_report)
        if not json_path.is_absolute():
            json_path = root / json_path
        json_path.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            "score": score,
            "grade": grade(score),
            "facts": asdict(facts),
            "findings": [asdict(f) for f in findings],
        }
        json_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")
        print(f"Wrote JSON report: {json_path}")

    errors = sum(1 for f in findings if f.severity == "ERROR")
    warnings = sum(1 for f in findings if f.severity == "WARN")
    print(f"Score: {score}/100 ({grade(score)}) — {errors} error(s), {warnings} warning(s)")
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
