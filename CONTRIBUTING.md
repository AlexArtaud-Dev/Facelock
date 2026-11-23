# Contribuer à FaceLock

## Commits et titres de PR

Tout suit [Conventional Commits](https://www.conventionalcommits.org/fr/), car c'est ce qui calcule la version et le changelog.

```
<type>(<portée optionnelle>): <résumé à l'impératif, en français>
```

| Type | Effet sur la version | Exemple |
|---|---|---|
| `feat` | mineure (x.**y**.0) | `feat(workflows): ajoute le déclencheur Retour` |
| `fix` | correctif (x.y.**z**) | `fix(camera): relâche la webcam après une erreur de lecture` |
| `perf`, `refactor`, `docs`, `build` | correctif | `perf(engine): évite une copie de l'image` |
| `test`, `ci`, `chore` | aucune (masqués du changelog) | `ci: épingle les actions par SHA` |
| `…!` ou pied `BREAKING CHANGE:` | majeure (**x**.0.0) | `feat(config)!: nouveau format des workflows` |

Portées courantes : `ui`, `workflows`, `actions`, `engine`, `camera`, `daemon`, `linux`, `windows`, `config`, `deps`.

Les PR sont fusionnées en **squash** : leur titre devient le message du commit et il est vérifié en CI.

## Release

1. Chaque fusion sur `main` met à jour la PR « chore(main): release x.y.z » ouverte par *release-please* (changelog et version de `Cargo.toml`).
2. Fusionner cette PR crée le tag `vx.y.z` et la release GitHub.
3. La CI construit et attache l'installeur Windows, l'exe portable, le `.deb`, l'archive Linux et les sommes SHA-256.

Une PR ouverte avec le jeton par défaut ne déclenche pas la CI. Un secret `RELEASE_TOKEN` (jeton *fine-grained* : contents et pull requests en écriture) règle ce problème.

## Avant d'ouvrir une PR

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Teste en vrai ce qui touche au système (caméra, raccourcis, tray, écran de confidentialité) sur le ou les OS concernés.
