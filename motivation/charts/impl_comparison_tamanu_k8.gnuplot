set terminal png size 1200,800 font "sans,10" enhanced
set output 'charts/impl_comparison_tamanu_k8.png'
set title 'Implementation Comparison: tamanu-k8 (100 requests, TCP) (lower is better)'
set xlabel 'Implementation'
set ylabel 'Duration (ms)'
set grid ytics
set key outside right top
set style data histograms
set style histogram clustered gap 1
set style fill solid 0.8 border -1
set boxwidth 0.9
set offset 0,0,graph 0.15,0
set xtics ("1" 0, "10" 1, "100" 2)


plot 'charts/impl_comparison_tamanu_k8_data.txt' using 2:xtic(1) title 'Duration' with boxes, \
     '' using 0:2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle
