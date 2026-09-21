# Packaging, signing, and distribution

`./scripts/build-app.sh` creates `dist/Pickle.app`, a runnable **locally ad-hoc signed development build**. It has a stable bundle ID (`com.pickle.reader`) and a menu-bar-only `LSUIElement` configuration. It is not a notarized release. Do not use the development signing method for public distribution.

## Release preparation

1. Have the release owner review/accept the Xcode license, enroll in the Apple Developer Program, and install their Developer ID Application certificate/private key.
2. Finalize the human-reviewed Jev evaluation, macOS compatibility matrix, privacy wording, application icon, version, deployment target and architecture support.
3. Build optimized code with `./scripts/build-app.sh release`. For an Intel+Apple Silicon universal build, configure appropriate target SDK/toolchain support, build both architectures, combine the executable with `lipo`, and test on both. The supplied binary was built/tested only on Apple Silicon.
4. Sign the complete app with hardened runtime using the owner’s identity. Native networking/Keychain/AX do not require arbitrary code-execution entitlements. If sandboxing is adopted later, revisit the cross-app Accessibility design and distribution requirements rather than blindly adding broad entitlements.
5. Submit a zipped app for notarization using credentials stored with `notarytool`, staple the accepted ticket, verify with `codesign`/`spctl`, and test on a clean Mac.

Example commands, **for the release owner after credentials and approvals are ready**:

```sh
codesign --force --options runtime --timestamp \
  --sign 'Developer ID Application: YOUR NAME (TEAMID)' dist/Pickle.app
/usr/bin/ditto -c -k --keepParent dist/Pickle.app dist/Pickle.zip
xcrun notarytool submit dist/Pickle.zip --keychain-profile YOUR_NOTARY_PROFILE --wait
xcrun stapler staple dist/Pickle.app
codesign --verify --strict --verbose=2 dist/Pickle.app
spctl --assess --type execute --verbose dist/Pickle.app
```

No certificate, notarization credential, billing method, or shared API key belongs in the repository. No signing/notarization/deployment was performed beyond local ad-hoc signing. The development app may need Accessibility permission re-added after each rebuild; a consistent signing identity improves permission and Keychain continuity.

## Install / remove

For development, launch `dist/Pickle.app` directly. To install later, copy the signed release app into `/Applications`, open it once, and grant Accessibility yourself.

Before uninstalling, clear the session and remove the saved API token in Pickle Settings. Quit the app, remove it from Applications, and revoke its Accessibility permission in System Settings. Preferences contain no selected text; Keychain stores only the user’s own credential. Persistent history is not included.
