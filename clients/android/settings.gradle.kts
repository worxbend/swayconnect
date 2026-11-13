pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "swayconnect-android"

include(":protocol")

// :app is written only after :protocol is green in CI (docs/IMPLEMENTATION_PLAN.md, S9).
if (settingsDir.resolve("app/build.gradle.kts").exists()) {
    include(":app")
}
