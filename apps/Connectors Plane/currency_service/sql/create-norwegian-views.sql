-- Norwegian Views for Power BI
-- Creates views that match the expected schema from existing dataflows

-- DataSammendrag (Data Summary)
CREATE OR REPLACE VIEW "DataSammendrag" AS
SELECT 
    COUNT(*) as "antall_kurser",
    COUNT(DISTINCT base) as "antall_valutaer",
    MIN(date) as "eldste_dato",
    MAX(date) as "nyeste_dato"
FROM "Rate";

-- Valutakurser (Currency Rates with Norwegian names)
CREATE OR REPLACE VIEW "Valutakurser" AS
SELECT 
    id,
    date as "kurs_dato",
    base as "valuta_kode",
    quote as "mot_valuta",
    CAST(value AS DECIMAL(18,4)) as "kurs_verdi",
    src as "kilde",
    CASE base
        WHEN 'USD' THEN 'Amerikanske dollar'
        WHEN 'EUR' THEN 'Euro'
        WHEN 'GBP' THEN 'Britiske pund'
        WHEN 'SEK' THEN 'Svenske kroner'
        WHEN 'DKK' THEN 'Danske kroner'
        WHEN 'CAD' THEN 'Kanadiske dollar'
        WHEN 'AUD' THEN 'Australske dollar'
        WHEN 'CHF' THEN 'Sveitsiske franc'
        WHEN 'JPY' THEN 'Japanske yen'
        WHEN 'ISK' THEN 'Islandske kroner'
        ELSE base
    END as "valuta_navn"
FROM "Rate"
WHERE quote = 'NOK';

-- SisteKurser (Latest Rates)
CREATE OR REPLACE VIEW "SisteKurser" AS
SELECT DISTINCT ON (base)
    base as "valuta_kode",
    quote as "mot_valuta",
    date as "kurs_dato",
    CAST(value AS DECIMAL(18,4)) as "kurs_verdi",
    src as "kilde",
    CASE base
        WHEN 'USD' THEN 'Amerikanske dollar'
        WHEN 'EUR' THEN 'Euro'
        WHEN 'GBP' THEN 'Britiske pund'
        WHEN 'SEK' THEN 'Svenske kroner'
        WHEN 'DKK' THEN 'Danske kroner'
        WHEN 'CAD' THEN 'Kanadiske dollar'
        WHEN 'AUD' THEN 'Australske dollar'
        WHEN 'CHF' THEN 'Sveitsiske franc'
        WHEN 'JPY' THEN 'Japanske yen'
        WHEN 'ISK' THEN 'Islandske kroner'
        ELSE base
    END as "valuta_navn"
FROM "Rate"
WHERE quote = 'NOK'
ORDER BY base, date DESC;

-- HistoricalRate (samme som Rate, men med norske kolonnenavn)
CREATE OR REPLACE VIEW "HistoricalRate" AS
SELECT 
    id,
    date as "dato",
    base as "fra_valuta",
    quote as "til_valuta",
    CAST(value AS DECIMAL(18,8)) as "verdi",
    src as "kilde"
FROM "Rate";
