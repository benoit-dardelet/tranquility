//! UI languages. English strings are the keys; each row gives French,
//! Spanish, German, Italian and Portuguese. Missing keys fall back to
//! English. Soft-key words are kept to 8 columns.
use crate::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    En,
    Fr,
    Es,
    De,
    It,
    Pt,
}

impl Lang {
    pub const ALL: [Lang; 6] = [Lang::En, Lang::Fr, Lang::Es, Lang::De, Lang::It, Lang::Pt];

    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "Français",
            Lang::Es => "Español",
            Lang::De => "Deutsch",
            Lang::It => "Italiano",
            Lang::Pt => "Português",
        }
    }
    pub fn code(self) -> u8 {
        self as u8
    }
    pub fn from_code(c: u64) -> Lang {
        Lang::ALL.get(c as usize).copied().unwrap_or_default()
    }
    fn col(self) -> Option<usize> {
        match self {
            Lang::En => None,
            l => Some(l as usize - 1),
        }
    }
}

/// Translate a static English string.
pub fn tr(l: Lang, s: &'static str) -> &'static str {
    match l.col() {
        None => s,
        Some(i) => TABLE.iter().find(|(k, _)| *k == s).map(|(_, v)| v[i]).unwrap_or(s),
    }
}

/// Translate a runtime string if it is a known key, else return it unchanged.
pub fn tr_dyn(l: Lang, s: &str) -> String {
    match l.col() {
        None => s.to_string(),
        Some(i) => TABLE.iter().find(|(k, _)| *k == s).map(|(_, v)| v[i].to_string()).unwrap_or_else(|| s.to_string()),
    }
}

/// Fill `{}` placeholders in order.
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = template;
    let mut it = args.iter();
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        out.push_str(it.next().copied().unwrap_or(""));
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

pub fn weekday(l: Lang, i: usize) -> &'static str {
    const W: [[&str; 7]; 6] = [
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
        ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."],
        ["dom", "lun", "mar", "mié", "jue", "vie", "sáb"],
        ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"],
        ["dom", "lun", "mar", "mer", "gio", "ven", "sab"],
        ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"],
    ];
    W[l as usize][i % 7]
}

pub fn month(l: Lang, i: usize) -> &'static str {
    const M: [[&str; 12]; 6] = [
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
        ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."],
        ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"],
        ["Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"],
        ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"],
        ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"],
    ];
    M[l as usize][i % 12]
}

/// Multi-tap letters for keys 0-9: the language's accented letters come
/// after the plain ones, before the digit.
pub fn keypad(l: Lang) -> [&'static str; 10] {
    let p = ".,?!1'\"-()@/:";
    match l {
        Lang::En => [" 0", p, "abc2", "def3", "ghi4", "jkl5", "mno6", "pqrs7", "tuv8", "wxyz9"],
        Lang::Fr => [" 0", ".,?!1'\"-()@/:«»", "abcàâç2", "deféèêë3", "ghiîï4", "jkl5", "mnoôœ6", "pqrs7", "tuvùûü8", "wxyz9"],
        Lang::Es => [" 0", ".,?!¿¡1'\"-()@/:", "abcá2", "defé3", "ghií4", "jkl5", "mnoñó6", "pqrs7", "tuvúü8", "wxyz9"],
        Lang::De => [" 0", p, "abcä2", "def3", "ghi4", "jkl5", "mnoö6", "pqrsß7", "tuvü8", "wxyz9"],
        Lang::It => [" 0", p, "abcà2", "defèé3", "ghiì4", "jkl5", "mnoò6", "pqrs7", "tuvù8", "wxyz9"],
        Lang::Pt => [" 0", p, "abcáàâãç2", "deféê3", "ghií4", "jkl5", "mnoóôõ6", "pqrs7", "tuvú8", "wxyz9"],
    }
}

pub const ABOUT: &str = "Tranquility OS 0.2\nA tiny Rust kernel for dumbphones.\n\nVoice (VoLTE/VoNR), SMS, MMS 1.3, RCS chat, USSD, cell broadcast alerts, 4G/5G.\n\nNo Linux, no Android.";

/// (English, [Français, Español, Deutsch, Italiano, Português])
pub static TABLE: &[(&str, [&str; 5])] = &[
    // soft keys (≤ 8 columns)
    ("Back", ["Retour", "Atrás", "Zurück", "Indietro", "Voltar"]),
    ("Select", ["Choisir", "Elegir", "Wählen", "Scegli", "Escolher"]),
    ("Clear", ["Effacer", "Borrar", "Löschen", "Cancella", "Apagar"]),
    ("Call", ["Appeler", "Llamar", "Anrufen", "Chiama", "Ligar"]),
    ("Options", ["Options", "Opciones", "Optionen", "Opzioni", "Opções"]),
    ("Delete", ["Supprim.", "Eliminar", "Löschen", "Elimina", "Excluir"]),
    ("Cancel", ["Annuler", "Cancelar", "Abbruch", "Annulla", "Cancelar"]),
    ("Save", ["Enreg.", "Guardar", "Sichern", "Salva", "Salvar"]),
    ("Send", ["Envoyer", "Enviar", "Senden", "Invia", "Enviar"]),
    ("Open", ["Ouvrir", "Abrir", "Öffnen", "Apri", "Abrir"]),
    ("OK", ["OK", "OK", "OK", "OK", "OK"]),
    ("Next", ["Suivant", "Seguir", "Weiter", "Avanti", "Seguinte"]),
    ("Menu", ["Menu", "Menú", "Menü", "Menu", "Menu"]),
    ("Names", ["Noms", "Nombres", "Namen", "Nomi", "Nomes"]),
    ("Write", ["Écrire", "Escribir", "Schreib.", "Scrivi", "Escrever"]),
    ("Message", ["Message", "Mensaje", "Schreib.", "Scrivi", "Mensagem"]),
    ("Change", ["Modifier", "Cambiar", "Ändern", "Modifica", "Alterar"]),
    ("Create", ["Créer", "Crear", "Anlegen", "Crea", "Criar"]),
    ("Answer", ["Répondre", "Atender", "Annehmen", "Rispondi", "Atender"]),
    ("Reject", ["Refuser", "Rechazar", "Ablehnen", "Rifiuta", "Recusar"]),
    ("Silence", ["Silence", "Silencio", "Stumm", "Silenzia", "Silêncio"]),
    ("Mute", ["Muet", "Mudo", "Stumm", "Muto", "Mudo"]),
    ("Unmute", ["Son", "Sonido", "Ton an", "Audio", "Som"]),
    ("Speaker", ["Haut-p.", "Altavoz", "Lautspr.", "Vivavoce", "Viva-voz"]),
    ("Handset", ["Combiné", "Normal", "Hörer", "Normale", "Normal"]),
    ("End", ["Fin", "Colgar", "Auflegen", "Chiudi", "Desligar"]),
    ("Snooze", ["Répéter", "Posponer", "Später", "Posponi", "Adiar"]),
    ("Stop", ["Arrêter", "Parar", "Stopp", "Ferma", "Parar"]),
    ("Start", ["Démarrer", "Iniciar", "Start", "Avvia", "Iniciar"]),
    ("Lap", ["Tour", "Vuelta", "Runde", "Giro", "Volta"]),
    ("Reset", ["RàZ", "Reinic.", "Reset", "Azzera", "Zerar"]),
    // titles and menus
    ("Messages", ["Messages", "Mensajes", "Nachrichten", "Messaggi", "Mensagens"]),
    ("Contacts", ["Contacts", "Contactos", "Kontakte", "Contatti", "Contatos"]),
    ("Call log", ["Journal d'appels", "Registro de llamadas", "Anrufliste", "Registro chiamate", "Registro de chamadas"]),
    ("Clock & alarm", ["Horloge et réveil", "Reloj y alarma", "Uhr & Wecker", "Orologio e sveglia", "Relógio e alarme"]),
    ("Calculator", ["Calculatrice", "Calculadora", "Rechner", "Calcolatrice", "Calculadora"]),
    ("Settings", ["Paramètres", "Ajustes", "Einstellungen", "Impostazioni", "Configurações"]),
    ("Network simulator", ["Simulateur réseau", "Simulador de red", "Netzsimulator", "Simulatore di rete", "Simulador de rede"]),
    ("Phone status", ["État du téléphone", "Estado del teléfono", "Telefonstatus", "Stato del telefono", "Estado do telefone"]),
    ("SIM locked", ["SIM verrouillée", "SIM bloqueada", "SIM gesperrt", "SIM bloccata", "SIM bloqueado"]),
    ("Enter SIM PIN", ["Saisir le code PIN", "Introduce el PIN", "SIM-PIN eingeben", "Inserisci il PIN", "Digite o PIN do SIM"]),
    ("{} attempts left", ["{} essais restants", "Quedan {} intentos", "Noch {} Versuche", "{} tentativi rimasti", "{} tentativas restantes"]),
    ("Emergency: dial 112", ["Urgence : composez 112", "Emergencia: marca 112", "Notruf: 112 wählen", "Emergenza: chiama 112", "Emergência: ligue 112"]),
    ("Wrong PIN", ["Code PIN erroné", "PIN incorrecto", "Falsche PIN", "PIN errato", "PIN incorreto"]),
    ("Airplane mode", ["Mode avion", "Modo avión", "Flugmodus", "Modalità aereo", "Modo avião"]),
    ("Searching…", ["Recherche…", "Buscando…", "Suche…", "Ricerca…", "Procurando…"]),
    ("No service", ["Aucun service", "Sin servicio", "Kein Netz", "Nessun servizio", "Sem serviço"]),
    ("✉ 1 new message", ["✉ 1 nouveau message", "✉ 1 mensaje nuevo", "✉ 1 neue Nachricht", "✉ 1 nuovo messaggio", "✉ 1 nova mensagem"]),
    ("✉ {} new messages", ["✉ {} nouveaux messages", "✉ {} mensajes nuevos", "✉ {} neue Nachrichten", "✉ {} nuovi messaggi", "✉ {} novas mensagens"]),
    ("☎ 1 missed call", ["☎ 1 appel manqué", "☎ 1 llamada perdida", "☎ 1 verpasster Anruf", "☎ 1 chiamata persa", "☎ 1 chamada perdida"]),
    ("☎ {} missed calls", ["☎ {} appels manqués", "☎ {} llamadas perdidas", "☎ {} verpasste Anrufe", "☎ {} chiamate perse", "☎ {} chamadas perdidas"]),
    ("◷ Alarm {}", ["◷ Réveil {}", "◷ Alarma {}", "◷ Wecker {}", "◷ Sveglia {}", "◷ Alarme {}"]),
    // simulator
    ("Simulator", ["Simulateur", "Simulador", "Simulator", "Simulatore", "Simulador"]),
    ("Incoming call (Bob)", ["Appel entrant (Bob)", "Llamada entrante (Bob)", "Anruf von Bob", "Chiamata da Bob", "Chamada de Bob"]),
    ("SMS from Bob", ["SMS de Bob", "SMS de Bob", "SMS von Bob", "SMS da Bob", "SMS de Bob"]),
    ("Chat from Alice", ["Chat d'Alice", "Chat de Alice", "Chat von Alice", "Chat da Alice", "Chat de Alice"]),
    ("MMS from Bob", ["MMS de Bob", "MMS de Bob", "MMS von Bob", "MMS da Bob", "MMS de Bob"]),
    ("Emergency alert", ["Alerte d'urgence", "Alerta de emergencia", "Notfallwarnung", "Allerta di emergenza", "Alerta de emergência"]),
    ("Lose signal 8s", ["Perte réseau 8 s", "Perder señal 8 s", "Netzverlust 8 s", "Perdi segnale 8 s", "Perder sinal 8 s"]),
    ("Long SMS (Carol)", ["SMS long (Carol)", "SMS largo (Carol)", "Lange SMS (Carol)", "SMS lungo (Carol)", "SMS longo (Carol)"]),
    ("Group chat (Alice)", ["Groupe (Alice)", "Grupo (Alice)", "Gruppe (Alice)", "Gruppo (Alice)", "Grupo (Alice)"]),
    ("Simulated event sent", ["Événement simulé envoyé", "Evento simulado enviado", "Ereignis gesendet", "Evento simulato inviato", "Evento simulado enviado"]),
    // phone status
    ("Network: {}", ["Réseau : {}", "Red: {}", "Netz: {}", "Rete: {}", "Rede: {}"]),
    ("none", ["aucun", "ninguna", "keins", "nessuna", "nenhuma"]),
    ("Signal: {}", ["Signal : {}", "Señal: {}", "Signal: {}", "Segnale: {}", "Sinal: {}"]),
    ("Data: {}", ["Données : {}", "Datos: {}", "Daten: {}", "Dati: {}", "Dados: {}"]),
    ("off", ["désactivé", "desactivado", "aus", "disattivati", "desligado"]),
    ("Battery: {}%", ["Batterie : {} %", "Batería: {} %", "Akku: {} %", "Batteria: {}%", "Bateria: {}%"]),
    ("Number: {}", ["Numéro : {}", "Número: {}", "Nummer: {}", "Numero: {}", "Número: {}"]),
    ("MMS sent/recv: {}", ["MMS env./reçus : {}", "MMS env./recib.: {}", "MMS ges./empf.: {}", "MMS inv./ric.: {}", "MMS env./rec.: {}"]),
    ("Saved data: {}", ["Sauvegarde : {}", "Guardado: {}", "Speicher: {}", "Salvataggio: {}", "Dados salvos: {}"]),
    ("Uptime: {}", ["Allumé depuis : {}", "Encendido: {}", "Laufzeit: {}", "Attivo da: {}", "Ligado há: {}"]),
    ("disk", ["disque", "disco", "Festplatte", "disco", "disco"]),
    ("not saved", ["non sauvegardé", "no se guarda", "nicht gespeichert", "non salvati", "não salvos"]),
    ("RAM (kept on restart)", ["RAM (conservé au redémarrage)", "RAM (se conserva al reiniciar)", "RAM (bleibt bei Neustart)", "RAM (resta al riavvio)", "RAM (mantido ao reiniciar)"]),
    // dialer and calls
    ("Dial", ["Composer", "Marcar", "Wählen", "Componi", "Discar"]),
    ("Service code", ["Code de service", "Código de servicio", "Dienstcode", "Codice servizio", "Código de serviço"]),
    ("Sending request…", ["Envoi de la demande…", "Enviando solicitud…", "Anfrage wird gesendet…", "Invio richiesta…", "Enviando pedido…"]),
    ("Call failed: {}", ["Échec de l'appel : {}", "Error en la llamada: {}", "Anruf fehlgeschlagen: {}", "Chiamata non riuscita: {}", "Falha na chamada: {}"]),
    ("No number", ["Aucun numéro", "Sin número", "Keine Nummer", "Nessun numero", "Sem número"]),
    ("Call in progress", ["Appel en cours", "Llamada en curso", "Anruf läuft", "Chiamata in corso", "Chamada em andamento"]),
    ("No network", ["Pas de réseau", "Sin red", "Kein Netz", "Nessuna rete", "Sem rede"]),
    ("Call ended", ["Appel terminé", "Llamada finalizada", "Anruf beendet", "Chiamata terminata", "Chamada encerrada"]),
    ("Busy", ["Occupé", "Ocupado", "Besetzt", "Occupato", "Ocupado"]),
    ("No answer", ["Pas de réponse", "Sin respuesta", "Keine Antwort", "Nessuna risposta", "Sem resposta"]),
    ("Unknown", ["Inconnu", "Desconocido", "Unbekannt", "Sconosciuto", "Desconhecido"]),
    ("Calling…", ["Appel…", "Llamando…", "Anruf…", "Chiamata…", "Chamando…"]),
    ("Ringing…", ["Sonnerie…", "Sonando…", "Klingelt…", "Squilla…", "Tocando…"]),
    ("Incoming call", ["Appel entrant", "Llamada entrante", "Eingehender Anruf", "Chiamata in arrivo", "Chamada recebida"]),
    ("Emergency call", ["Appel d'urgence", "Llamada de emergencia", "Notruf", "Chiamata di emergenza", "Chamada de emergência"]),
    ("Voice call", ["Appel vocal", "Llamada de voz", "Sprachanruf", "Chiamata vocale", "Chamada de voz"]),
    ("Muted", ["Muet", "Silenciado", "Stumm", "Muto", "Mudo"]),
    // messaging
    ("✎ New message", ["✎ Nouveau message", "✎ Nuevo mensaje", "✎ Neue Nachricht", "✎ Nuovo messaggio", "✎ Nova mensagem"]),
    ("✎ New group", ["✎ Nouveau groupe", "✎ Nuevo grupo", "✎ Neue Gruppe", "✎ Nuovo gruppo", "✎ Novo grupo"]),
    ("[picture]", ["[image]", "[imagen]", "[Bild]", "[immagine]", "[imagem]"]),
    ("Delete conversation?", ["Supprimer la conversation ?", "¿Eliminar conversación?", "Unterhaltung löschen?", "Eliminare la conversazione?", "Excluir conversa?"]),
    ("Conversation deleted", ["Conversation supprimée", "Conversación eliminada", "Unterhaltung gelöscht", "Conversazione eliminata", "Conversa excluída"]),
    ("New message", ["Nouveau message", "Nuevo mensaje", "Neue Nachricht", "Nuovo messaggio", "Nova mensagem"]),
    ("To: {}_", ["À : {}_", "Para: {}_", "An: {}_", "A: {}_", "Para: {}_"]),
    ("Type a number, or clear", ["Tapez un numéro, ou", "Escribe un número, o", "Nummer eingeben oder", "Digita un numero, o", "Digite um número, ou"]),
    ("it to pick a contact.", ["effacez pour un contact.", "bórralo para un contacto.", "löschen für Kontakte.", "cancella per i contatti.", "apague para um contato."]),
    ("Group name", ["Nom du groupe", "Nombre del grupo", "Gruppenname", "Nome del gruppo", "Nome do grupo"]),
    ("{} members + you", ["{} membres + vous", "{} miembros + tú", "{} Mitglieder + du", "{} membri + te", "{} membros + você"]),
    ("New group ({})", ["Nouveau groupe ({})", "Nuevo grupo ({})", "Neue Gruppe ({})", "Nuovo gruppo ({})", "Novo grupo ({})"]),
    ("Read", ["Lu", "Leído", "Gelesen", "Letto", "Lida"]),
    ("! Failed", ["! Échec", "! Error", "! Fehler", "! Non inviato", "! Falhou"]),
    (" [Contact] {}", [" [Contact] {}", " [Contacto] {}", " [Kontakt] {}", " [Contatto] {}", " [Contato] {}"]),
    ("Me", ["Moi", "Yo", "Ich", "Io", "Eu"]),
    (" Subj: {}", [" Objet : {}", " Asunto: {}", " Betreff: {}", " Oggetto: {}", " Assunto: {}"]),
    ("{} is typing…", ["{} écrit…", "{} está escribiendo…", "{} schreibt…", "{} sta scrivendo…", "{} está digitando…"]),
    ("Group", ["Groupe", "Grupo", "Gruppe", "Gruppo", "Grupo"]),
    ("Reply", ["Répondre", "Responder", "Antworten", "Rispondi", "Responder"]),
    ("Members", ["Membres", "Miembros", "Mitglieder", "Membri", "Membros"]),
    ("Delete conversation", ["Supprimer la conversation", "Eliminar conversación", "Unterhaltung löschen", "Elimina conversazione", "Excluir conversa"]),
    ("Check chat features", ["Vérifier le chat", "Comprobar chat", "Chat prüfen", "Verifica chat", "Verificar chat"]),
    ("You", ["Vous", "Tú", "Du", "Tu", "Você"]),
    ("◆ = can use chat", ["◆ = peut utiliser le chat", "◆ = puede usar el chat", "◆ = kann chatten", "◆ = può usare la chat", "◆ = pode usar o chat"]),
    ("Checking…", ["Vérification…", "Comprobando…", "Prüfe…", "Verifica…", "Verificando…"]),
    ("To {}", ["À {}", "Para {}", "An {}", "A {}", "Para {}"]),
    ("Group MMS", ["MMS de groupe", "MMS de grupo", "Gruppen-MMS", "MMS di gruppo", "MMS em grupo"]),
    ("MMS · 1 attachment", ["MMS · 1 pièce jointe", "MMS · 1 adjunto", "MMS · 1 Anhang", "MMS · 1 allegato", "MMS · 1 anexo"]),
    ("MMS · {} attachments", ["MMS · {} pièces jointes", "MMS · {} adjuntos", "MMS · {} Anhänge", "MMS · {} allegati", "MMS · {} anexos"]),
    ("Group chat", ["Chat de groupe", "Chat de grupo", "Gruppenchat", "Chat di gruppo", "Chat em grupo"]),
    ("Chat · file", ["Chat · fichier", "Chat · archivo", "Chat · Datei", "Chat · file", "Chat · arquivo"]),
    ("Chat message", ["Message chat", "Mensaje de chat", "Chat-Nachricht", "Messaggio chat", "Mensagem de chat"]),
    ("Insert picture: smiley", ["Image : smiley", "Imagen: sonrisa", "Bild: Smiley", "Immagine: smile", "Imagem: sorriso"]),
    ("Insert picture: heart", ["Image : cœur", "Imagen: corazón", "Bild: Herz", "Immagine: cuore", "Imagem: coração"]),
    ("Insert contact card", ["Carte de visite", "Tarjeta de contacto", "Visitenkarte", "Biglietto da visita", "Cartão de contato"]),
    ("Use chat if available", ["Chat si disponible", "Chat si está disponible", "Chat wenn möglich", "Chat se disponibile", "Chat se disponível"]),
    ("Send as SMS", ["Envoyer en SMS", "Enviar como SMS", "Als SMS senden", "Invia come SMS", "Enviar como SMS"]),
    ("Remove attachments", ["Retirer les pièces jointes", "Quitar adjuntos", "Anhänge entfernen", "Rimuovi allegati", "Remover anexos"]),
    ("Message from {}", ["Message de {}", "Mensaje de {}", "Nachricht von {}", "Messaggio da {}", "Mensagem de {}"]),
    ("MMS from {}", ["MMS de {}", "MMS de {}", "MMS von {}", "MMS da {}", "MMS de {}"]),
    // contacts
    ("+ Add contact", ["+ Ajouter un contact", "+ Añadir contacto", "+ Kontakt hinzufügen", "+ Aggiungi contatto", "+ Adicionar contato"]),
    ("Send message", ["Envoyer un message", "Enviar mensaje", "Nachricht senden", "Invia messaggio", "Enviar mensagem"]),
    ("Edit", ["Modifier", "Editar", "Bearbeiten", "Modifica", "Editar"]),
    ("Chat: available", ["Chat : disponible", "Chat: disponible", "Chat: verfügbar", "Chat: disponibile", "Chat: disponível"]),
    ("Chat: not available", ["Chat : indisponible", "Chat: no disponible", "Chat: nicht verfügbar", "Chat: non disponibile", "Chat: indisponível"]),
    ("Chat: unknown", ["Chat : inconnu", "Chat: desconocido", "Chat: unbekannt", "Chat: sconosciuto", "Chat: desconhecido"]),
    ("Chat features are off", ["Le chat est désactivé", "El chat está desactivado", "Chat ist ausgeschaltet", "La chat è disattivata", "O chat está desativado"]),
    ("Delete contact?", ["Supprimer le contact ?", "¿Eliminar contacto?", "Kontakt löschen?", "Eliminare il contatto?", "Excluir contato?"]),
    ("Contact deleted", ["Contact supprimé", "Contacto eliminado", "Kontakt gelöscht", "Contatto eliminato", "Contato excluído"]),
    ("Edit contact", ["Modifier le contact", "Editar contacto", "Kontakt bearbeiten", "Modifica contatto", "Editar contato"]),
    ("New contact", ["Nouveau contact", "Nuevo contacto", "Neuer Kontakt", "Nuovo contatto", "Novo contato"]),
    ("Name: ({})", ["Nom : ({})", "Nombre: ({})", "Name: ({})", "Nome: ({})", "Nome: ({})"]),
    ("Number:", ["Numéro :", "Número:", "Nummer:", "Numero:", "Número:"]),
    ("Name and number needed", ["Nom et numéro requis", "Faltan nombre y número", "Name und Nummer nötig", "Servono nome e numero", "Nome e número obrigatórios"]),
    ("Contact saved", ["Contact enregistré", "Contacto guardado", "Kontakt gespeichert", "Contatto salvato", "Contato salvo"]),
    ("No calls yet", ["Aucun appel", "Sin llamadas", "Noch keine Anrufe", "Nessuna chiamata", "Nenhuma chamada"]),
    // clock
    ("Clock", ["Horloge", "Reloj", "Uhr", "Orologio", "Relógio"]),
    ("Alarm {}  [{}]", ["Réveil {}  [{}]", "Alarma {}  [{}]", "Wecker {}  [{}]", "Sveglia {}  [{}]", "Alarme {}  [{}]"]),
    ("On", ["Oui", "Sí", "An", "Sì", "Sim"]),
    ("Off", ["Non", "No", "Aus", "No", "Não"]),
    ("Set alarm time", ["Régler le réveil", "Ajustar alarma", "Weckzeit stellen", "Imposta sveglia", "Definir alarme"]),
    ("Stopwatch", ["Chronomètre", "Cronómetro", "Stoppuhr", "Cronometro", "Cronômetro"]),
    ("Set alarm", ["Régler le réveil", "Ajustar alarma", "Wecker stellen", "Imposta sveglia", "Definir alarme"]),
    ("Type HHMM (24h)", ["Tapez HHMM (24 h)", "Escribe HHMM (24 h)", "HHMM eingeben (24 h)", "Digita HHMM (24 h)", "Digite HHMM (24 h)"]),
    ("Invalid time", ["Heure invalide", "Hora no válida", "Ungültige Zeit", "Ora non valida", "Hora inválida"]),
    ("Alarm set for {}", ["Réveil réglé à {}", "Alarma a las {}", "Wecker auf {} gestellt", "Sveglia alle {}", "Alarme para {}"]),
    ("Alarm", ["Réveil", "Alarma", "Wecker", "Sveglia", "Alarme"]),
    ("WAKE UP", ["DEBOUT", "DESPIERTA", "AUFSTEHEN", "SVEGLIA", "ACORDE"]),
    ("Snoozed for 5 minutes", ["Répété dans 5 minutes", "Pospuesta 5 minutos", "In 5 Minuten wieder", "Posposta di 5 minuti", "Adiado por 5 minutos"]),
    ("Lap {}  {}", ["Tour {}  {}", "Vuelta {}  {}", "Runde {}  {}", "Giro {}  {}", "Volta {}  {}"]),
    ("Error", ["Erreur", "Error", "Fehler", "Errore", "Erro"]),
    // settings
    ("Language: {}", ["Langue : {}", "Idioma: {}", "Sprache: {}", "Lingua: {}", "Idioma: {}"]),
    ("Language", ["Langue", "Idioma", "Sprache", "Lingua", "Idioma"]),
    ("Airplane mode: {}", ["Mode avion : {}", "Modo avión: {}", "Flugmodus: {}", "Mod. aereo: {}", "Modo avião: {}"]),
    ("Chat features: {}", ["Chat : {}", "Chat: {}", "Chat: {}", "Chat: {}", "Chat: {}"]),
    (" Status: {}", [" État : {}", " Estado: {}", " Status: {}", " Stato: {}", " Estado: {}"]),
    ("Delivery reports: {}", ["Accusés récept. : {}", "Informes entrega: {}", "Zustellberichte: {}", "Conferme consegna: {}", "Relat. de entrega: {}"]),
    ("Read receipts: {}", ["Confirm. lecture : {}", "Confirm. lectura: {}", "Lesebestätig.: {}", "Conferme lettura: {}", "Confirm. leitura: {}"]),
    ("Typing indicator: {}", ["Indic. de saisie : {}", "Indic. escritura: {}", "Tippanzeige: {}", "Indic. scrittura: {}", "Indic. digitação: {}"]),
    ("Ringtone: {}", ["Sonnerie : {}", "Tono: {}", "Klingelton: {}", "Suoneria: {}", "Toque: {}"]),
    ("AT command log", ["Journal AT", "Registro AT", "AT-Protokoll", "Registro AT", "Registro AT"]),
    ("About", ["À propos", "Acerca de", "Über", "Info", "Sobre"]),
    ("Restart phone", ["Redémarrer", "Reiniciar", "Neu starten", "Riavvia", "Reiniciar"]),
    ("Power off", ["Éteindre", "Apagar", "Ausschalten", "Spegni", "Desligar"]),
    ("Restart phone?", ["Redémarrer ?", "¿Reiniciar?", "Neu starten?", "Riavviare?", "Reiniciar?"]),
    ("Restart", ["Redémarrer", "Reiniciar", "Neu starten", "Riavvia", "Reiniciar"]),
    ("Power off?", ["Éteindre ?", "¿Apagar?", "Ausschalten?", "Spegnere?", "Desligar?"]),
    ("Network mode", ["Mode réseau", "Modo de red", "Netzmodus", "Modalità rete", "Modo de rede"]),
    ("5G/4G auto", ["5G/4G auto", "5G/4G auto", "5G/4G auto", "5G/4G auto", "5G/4G auto"]),
    ("4G only", ["4G uniquement", "Solo 4G", "Nur 4G", "Solo 4G", "Só 4G"]),
    ("5G only", ["5G uniquement", "Solo 5G", "Nur 5G", "Solo 5G", "Só 5G"]),
    ("Disconnected", ["Déconnecté", "Desconectado", "Getrennt", "Disconnesso", "Desconectado"]),
    ("Connecting", ["Connexion", "Conectando", "Verbinde", "Connessione", "Conectando"]),
    ("Registering", ["Inscription", "Registrando", "Anmeldung", "Registrazione", "Registrando"]),
    ("Connected", ["Connecté", "Conectado", "Verbunden", "Connesso", "Conectado"]),
    ("Error, retrying", ["Erreur, nouvel essai", "Error, reintentando", "Fehler, neuer Versuch", "Errore, riprovo", "Erro, tentando"]),
    (
        ABOUT,
        [
            "Tranquility OS 0.2\nUn petit noyau Rust pour téléphones simples.\n\nVoix (VoLTE/VoNR), SMS, MMS 1.3, chat RCS, USSD, alertes d'urgence, 4G/5G.\n\nNi Linux, ni Android.",
            "Tranquility OS 0.2\nUn pequeño núcleo en Rust para teléfonos básicos.\n\nVoz (VoLTE/VoNR), SMS, MMS 1.3, chat RCS, USSD, alertas de emergencia, 4G/5G.\n\nSin Linux, sin Android.",
            "Tranquility OS 0.2\nEin kleiner Rust-Kernel für einfache Handys.\n\nSprache (VoLTE/VoNR), SMS, MMS 1.3, RCS-Chat, USSD, Notfallwarnungen, 4G/5G.\n\nKein Linux, kein Android.",
            "Tranquility OS 0.2\nUn piccolo kernel Rust per telefoni semplici.\n\nVoce (VoLTE/VoNR), SMS, MMS 1.3, chat RCS, USSD, allerte di emergenza, 4G/5G.\n\nNiente Linux, niente Android.",
            "Tranquility OS 0.2\nUm pequeno kernel em Rust para celulares simples.\n\nVoz (VoLTE/VoNR), SMS, MMS 1.3, chat RCS, USSD, alertas de emergência, 4G/5G.\n\nSem Linux, sem Android.",
        ],
    ),
    // network / alerts
    ("Network", ["Réseau", "Red", "Netz", "Rete", "Rede"]),
    ("⚠ EMERGENCY ALERT ⚠", ["⚠ ALERTE D'URGENCE ⚠", "⚠ ALERTA EMERGENCIA ⚠", "⚠ NOTFALLWARNUNG ⚠", "⚠ ALLERTA EMERGENZA ⚠", "⚠ ALERTA EMERGÊNCIA ⚠"]),
    ("Presidential alert", ["Alerte présidentielle", "Alerta presidencial", "Präsidentenwarnung", "Allerta presidenziale", "Alerta presidencial"]),
    ("Extreme alert", ["Alerte extrême", "Alerta extrema", "Extreme Warnung", "Allerta estrema", "Alerta extremo"]),
    ("Severe alert", ["Alerte grave", "Alerta grave", "Schwere Warnung", "Allerta grave", "Alerta grave"]),
    ("AMBER alert", ["Alerte enlèvement", "Alerta AMBER", "AMBER-Alarm", "Allerta AMBER", "Alerta AMBER"]),
    ("Required monthly test", ["Test mensuel", "Prueba mensual", "Monatlicher Test", "Test mensile", "Teste mensal"]),
    ("Exercise", ["Exercice", "Simulacro", "Übung", "Esercitazione", "Exercício"]),
    ("Public safety", ["Sécurité publique", "Seguridad pública", "Öffentliche Sicherheit", "Sicurezza pubblica", "Segurança pública"]),
    ("Cell broadcast", ["Diffusion cellulaire", "Difusión celular", "Cell Broadcast", "Cell broadcast", "Difusão celular"]),
    ("Earthquake warning", ["Alerte séisme", "Alerta de terremoto", "Erdbebenwarnung", "Allerta terremoto", "Alerta de terremoto"]),
    ("Tsunami warning", ["Alerte tsunami", "Alerta de tsunami", "Tsunamiwarnung", "Allerta tsunami", "Alerta de tsunami"]),
    ("Earthquake & tsunami", ["Séisme et tsunami", "Terremoto y tsunami", "Erdbeben & Tsunami", "Terremoto e tsunami", "Terremoto e tsunami"]),
    ("Emergency warning", ["Avertissement d'urgence", "Aviso de emergencia", "Notfallhinweis", "Avviso di emergenza", "Aviso de emergência"]),
    ("USSD request failed", ["Échec de la demande USSD", "Error en la solicitud USSD", "USSD-Anfrage fehlgeschlagen", "Richiesta USSD non riuscita", "Falha no pedido USSD"]),
    ("USSD session ended", ["Session USSD terminée", "Sesión USSD finalizada", "USSD-Sitzung beendet", "Sessione USSD terminata", "Sessão USSD encerrada"]),
    // toasts from services / platform
    ("No SIM", ["Pas de SIM", "Sin SIM", "Keine SIM", "Nessuna SIM", "Sem SIM"]),
    ("Chat features connected", ["Chat connecté", "Chat conectado", "Chat verbunden", "Chat connessa", "Chat conectado"]),
    ("Chat features disconnected", ["Chat déconnecté", "Chat desconectado", "Chat getrennt", "Chat disconnessa", "Chat desconectado"]),
    ("Chat: authentication failed", ["Chat : échec d'authentification", "Chat: fallo de autenticación", "Chat: Anmeldung fehlgeschlagen", "Chat: autenticazione fallita", "Chat: falha na autenticação"]),
    ("Chat: registration rejected", ["Chat : inscription refusée", "Chat: registro rechazado", "Chat: Registrierung abgelehnt", "Chat: registrazione rifiutata", "Chat: registro recusado"]),
    ("Chat: registration timed out", ["Chat : délai dépassé", "Chat: tiempo agotado", "Chat: Zeitüberschreitung", "Chat: tempo scaduto", "Chat: tempo esgotado"]),
    ("MMS download failed", ["Échec du téléchargement MMS", "Error al descargar MMS", "MMS-Download fehlgeschlagen", "Download MMS non riuscito", "Falha ao baixar MMS"]),
    ("Storage full: delete old conversations", ["Mémoire pleine : supprimez des conversations", "Memoria llena: borra conversaciones", "Speicher voll: alte Chats löschen", "Memoria piena: elimina conversazioni", "Memória cheia: apague conversas"]),
    ("Couldn't save data: not restarting", ["Sauvegarde impossible : pas de redémarrage", "No se pudo guardar: no se reinicia", "Speichern fehlgeschlagen: kein Neustart", "Salvataggio fallito: niente riavvio", "Falha ao salvar: sem reinício"]),
    ("Restart not supported by firmware", ["Redémarrage non pris en charge", "Reinicio no compatible", "Neustart nicht unterstützt", "Riavvio non supportato", "Reinício não suportado"]),
    ("Restart not available: hold the power button", ["Redémarrage indisponible : maintenez le bouton", "Reinicio no disponible: mantén el botón", "Neustart nicht möglich: Taste halten", "Riavvio non disponibile: tieni premuto", "Reinício indisponível: segure o botão"]),
];

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn table_is_complete_and_unique() {
        for (i, (k, v)) in TABLE.iter().enumerate() {
            assert!(TABLE[..i].iter().all(|(k2, _)| k2 != k), "duplicate key {:?}", k);
            for t in v {
                assert!(!t.is_empty(), "{:?}", k);
                assert_eq!(k.matches("{}").count(), t.matches("{}").count(), "placeholders differ for {:?} → {:?}", k, t);
            }
        }
    }

    #[test]
    fn soft_keys_fit() {
        for k in ["Back", "Select", "Clear", "Call", "Options", "Delete", "Cancel", "Save", "Send", "Open", "Next", "Menu", "Names", "Write", "Message", "Change", "Create", "Answer", "Reject", "Silence", "Mute", "Unmute", "Speaker", "Handset", "End", "Snooze", "Stop", "Start", "Lap", "Reset"] {
            for l in Lang::ALL {
                assert!(crate::util::char_len(tr(l, k)) <= 8, "{:?} {:?} = {:?}", l, k, tr(l, k));
            }
        }
    }

    #[test]
    fn every_ui_string_is_translated() {
        // Scan the sources for strings passed to the translation helpers.
        let sources = [include_str!("screens.rs"), include_str!("mod.rs"), include_str!("../services/mod.rs"), include_str!("../services/rcs.rs"), include_str!("../services/mms.rs")];
        let mut missing = Vec::new();
        for src in sources {
            for pat in ["t(c, \"", "tf(c, \"", "toast(\"", "tr(l, \"", "self.t(\""] {
                let mut rest = src;
                while let Some(i) = rest.find(pat) {
                    rest = &rest[i + pat.len()..];
                    let end = rest.find('"').unwrap();
                    let key = &rest[..end];
                    if !key.is_empty() && !TABLE.iter().any(|(k, _)| *k == key) && !missing.contains(&key.to_string()) {
                        missing.push(key.to_string());
                    }
                }
            }
            // Menu::new("title", &["a", "b"], tag) lines
            for line in src.lines().filter(|l| l.contains("Menu::new(") || l.trim_start().starts_with("&[\"")) {
                for (i, part) in line.split('"').enumerate() {
                    if i % 2 == 1 && !part.is_empty() && part.chars().any(|c| c.is_alphabetic()) && !TABLE.iter().any(|(k, _)| *k == part) && !missing.contains(&part.to_string()) {
                        missing.push(part.to_string());
                    }
                }
            }
        }
        assert!(missing.is_empty(), "untranslated: {:#?}", missing);
    }

    #[test]
    fn fill_and_fallback() {
        assert_eq!(fill("{} of {}", &["1", "2"]), "1 of 2");
        assert_eq!(tr(Lang::Fr, "Back"), "Retour");
        assert_eq!(tr(Lang::De, "not a key"), "not a key");
        assert_eq!(tr_dyn(Lang::Es, "Busy"), "Ocupado");
    }
}
