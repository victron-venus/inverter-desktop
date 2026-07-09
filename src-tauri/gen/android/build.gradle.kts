buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        val agpVersion = "8.11.0"
        val kotlinPluginVersion = "1.9.25"
        classpath("com.android.tools.build:gradle:$agpVersion")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:$kotlinPluginVersion")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

tasks.register("clean").configure {
    delete("build")
}

