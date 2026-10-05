pluginManagement {
    repositories { google(); mavenCentral(); gradlePluginPortal(); maven("https://jitpack.io") }
}
dependencyResolutionManagement {
    repositories { google(); mavenCentral(); maven("https://jitpack.io") }
}
rootProject.name = "rust-renderer-plugin"
include(":app")
include(":renderer")
