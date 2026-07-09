plugins {
    `kotlin-dsl`
}

gradlePlugin {
    plugins {
        create("pluginsForCoolKids") {
            id = "rust"
            implementationClass = "RustPlugin"
        }
    }
}

repositories {
    google()
    mavenCentral()
}

val agpVersion = "8.11.0"

dependencies {
    compileOnly(gradleApi())
    implementation("com.android.tools.build:gradle:$agpVersion")
}

