# The Rust library calls nothing back into Java, but Kotlin calls it by name.
-keepclasseswithmembernames class * { native <methods>; }
