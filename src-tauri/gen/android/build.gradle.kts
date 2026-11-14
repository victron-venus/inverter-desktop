buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        val agpVersion = "8.13.2"
        val kotlinPluginVersion = "2.3.21"
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
