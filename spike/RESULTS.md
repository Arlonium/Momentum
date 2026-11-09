# Mementum — résultats de la phase 0

Mesures du spike. PC = poste Windows de Lenny (i9-14900K), ARM = Raspberry Pi 4 (4 Go) ou VPS ARM64 2 vCPU / 4 Go.

| Mesure | Budget | PC | ARM | Verdict |
|---|---|---|---|---|
| T1 · Chargement serveur d'une page de 10 000 blocs depuis SQLite (p95, 20 essais) | page jamais vue < 300 ms | 31,2 ms (p50 28,6) | à faire | ✅ provisoire |
| T1 · Démarrage à froid du binaire jusqu'à `/health` | < 1 s | 72 ms | à faire | ✅ provisoire |
| T1 · Mémoire du serveur au repos (RSS) | < 150 Mo | 8,6 Mo | à faire | ✅ provisoire |
| T1 · Propagation A → B via le serveur (test `two_clients_converge`) | < 150 ms | < 150 ms (assertion) | — | ✅ |

Notes T1 :
- L'instantané d'une page de 10 000 blocs pèse ~2,1 Mo.
- Les poignées de main d'un document vide (mise à jour sans effet) ne sont ni stockées ni diffusées.
- yrs doit être compilé avec la feature `sync` pour partager `Awareness` entre tâches tokio.
