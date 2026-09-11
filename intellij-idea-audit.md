# IntelliJ IDEA Project Audit

**Project:** `/home/paul/Projects/yt-dlp`  
**Score:** **82/100 — Good**  
**Findings:** 0 error(s), 3 warning(s), 0 info item(s), 10 pass(es)

## Project facts

| Item | Value |
|---|---|
| Git repository | Yes |
| Build systems | Node/npm, Python |
| Languages detected | JavaScript, Python, Rust, TypeScript |
| `.idea/` | Present |
| `.run/` | Absent |
| Tracked files | 251 |
| Modules detected | None / not inferred |
| Java/JDK signals | None detected |

## Priority findings

### [WARN] IntelliJ .iml module files are tracked

**Category:** Git & portability

For Maven/Gradle projects, module metadata is usually better regenerated from the build model. Tracking .iml can cause IDE-version and machine-specific churn.

**Paths:** `yt-dlp.iml`

**Recommendation:** If this is a Maven/Gradle project, prefer ignoring *.iml and import modules from the build tool. Keep them only when the IDE project model itself is authoritative.

### [WARN] IDE team settings exist but are not tracked

**Category:** IntelliJ IDEA

Shareable project settings appear on disk but not in Git, so other developers and CI helpers will not receive them.

**Recommendation:** Commit only stable, team-relevant IntelliJ project settings; keep per-user state ignored.

### [WARN] Possible absolute user paths in IntelliJ metadata

**Category:** IntelliJ IDEA

Absolute home-directory paths can make shared IDE configuration non-portable.

**Paths:** `.idea/workspace.xml`

**Recommendation:** Replace machine-specific paths with $PROJECT_DIR$, $MODULE_DIR$, environment variables, or build-tool configuration where possible.

## All checks

| Severity | Category | Check | Detail |
|---|---|---|---|

| WARN | Git & portability | IntelliJ .iml module files are tracked | For Maven/Gradle projects, module metadata is usually better regenerated from the build model. Tracking .iml can cause IDE-version and machine-specific churn. |

| WARN | IntelliJ IDEA | IDE team settings exist but are not tracked | Shareable project settings appear on disk but not in Git, so other developers and CI helpers will not receive them. |

| WARN | IntelliJ IDEA | Possible absolute user paths in IntelliJ metadata | Absolute home-directory paths can make shared IDE configuration non-portable. |

| PASS | CI reproducibility | CI configuration detected | Detected: .github/workflows |

| PASS | Code style | .editorconfig present | EditorConfig provides IDE-independent formatting basics and IntelliJ IDEA supports it. |

| PASS | Code style | Shared IntelliJ inspection profile detected | Project inspection settings are available to the team. |

| PASS | Git & portability | .gitignore covers common IDE/build outputs | Common IntelliJ and build-output patterns appear to be covered. |

| PASS | Git & portability | Git repository detected | 251 tracked files were inspected. |

| PASS | Git & portability | No common machine-local IntelliJ state is tracked | Common per-user IntelliJ files were not found in Git. |

| PASS | IntelliJ IDEA | .idea directory detected | IntelliJ project metadata is present. |

| PASS | IntelliJ IDEA | Git VCS mapping configured | .idea/vcs.xml maps the project to Git. |

| PASS | IntelliJ IDEA | Reusable team IDE configuration found | Detected: .idea/codeStyles (project code style), .idea/inspectionProfiles (inspection profile), .idea/runConfigurations (legacy shared run configurations) |

| PASS | Run/debug | Shared run/debug configurations detected | Found 12 run configuration file(s). |

## Recommended IntelliJ/Git policy

A practical default for Maven/Gradle repositories is:

- Treat the build tool as the source of truth for dependencies, modules, source roots, generated sources, compiler level, annotation processors, and test layout.
- Share only stable IntelliJ project settings that improve team consistency, such as project code style, inspection profiles, and genuinely reusable run/debug configurations.
- Do **not** commit user/session state such as `workspace.xml`, shelves, local HTTP requests, or local data-source state.
- Prefer `.run/*.run.xml` or shared project run configurations for reusable launches; keep secrets in environment variables or secret-management tooling.
- Prefer `.editorconfig` for portable baseline formatting, supplemented by IntelliJ-specific code-style settings only where useful.
- Use Maven/Gradle wrappers and explicit JDK/toolchain configuration so IntelliJ and CI resolve the same build model.
- For Maven/Gradle projects, normally let IntelliJ regenerate `*.iml` files instead of versioning them.

## Suggested `.gitignore` baseline

```gitignore
# IntelliJ IDEA: keep stable team settings, ignore user/session state
.idea/workspace.xml
.idea/tasks.xml
.idea/usage.statistics.xml
.idea/shelf/
.idea/httpRequests/
.idea/dataSources.local.xml

# IntelliJ generated module/output files
*.iml
out/

# OS noise
.DS_Store
Thumbs.db

# Optional local env/secrets
.env
.env.local
```

## Interpretation

This audit is heuristic. IntelliJ IDEA supports many valid project models, and some repositories intentionally version more or less IDE metadata. A finding should be judged against whether Maven/Gradle/Bazel or the IntelliJ project model is intended to be authoritative.
