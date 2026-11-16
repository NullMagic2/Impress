# Impress Hunspell adapter

Compiles Hunspell 1.7.2 from the adjacent source directory, downloaded from the official v1.7.2 release. The source is distributed under its MPL 1.1/GPL 2.0/LGPL 2.1 tri-license; Impress uses the MPL 1.1 option. Hunspell's license files and MySpell notices are retained in that directory and shipped with binaries.

Modification: `csutil.cxx::myopen` uses C++17 `std::filesystem::u8path` on all platforms so dictionary paths support non-ASCII Windows account names. All C API calls are serialized to protect upstream global Unicode data. The Rust adapter converts between Unicode and each dictionary's declared encoding.
